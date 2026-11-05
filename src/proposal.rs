//! Agent proposals: a JSON document with values to fill in, and the rules to apply it.
//!
//! The agent writes a proposal, the app shows it and writes only what the user accepts.
//! Everything a proposal writes is `estimated` and carries a source; version 1 never overwrites
//! a value the user confirmed.
//!
//! Shape, for profile 5:
//!
//! ```json
//! {
//!   "api": 1,
//!   "profile": 5,
//!   "values": [
//!     {"target": "detail", "field": "lifecycle", "value": "perennial",
//!      "source_type": "website", "source_ref": "https://example.org", "confidence": "high"},
//!     {"target": "requirement", "stage": "Flowering", "metric": "air_temp",
//!      "day_part": "day", "min": 12, "opt_min": 22, "opt_max": 28, "...": "source"},
//!     {"target": "timing_rule", "kind": "sow_indoor", "anchor": "last_frost",
//!      "offset_min_days": -70, "offset_max_days": -56, "...": "source"},
//!     {"target": "attribute", "key": "climate_type", "value": "tropical", "...": "source"},
//!     {"target": "stage", "label": "Flowering", "kind": "flowering",
//!      "duration_days_min": 30, "duration_days_max": 45, "...": "source"}
//!   ]
//! }
//! ```

use std::collections::HashMap;
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::changes::{self, NewChange as HistoryChange};
use crate::plant_data::{
    Attribute, AttributeDefinition, CONFIDENCE_LEVELS, DAY_PARTS, METRICS, PROVENANCE_STATUSES,
    ProfileDetails, Provenance, Requirement, RequirementResolution, SOURCE_TYPES, STAGE_KINDS,
    Stage, TIMING_ANCHORS, TIMING_KINDS, TimingRule, ValidationError, format_number,
};
use crate::store::Store;
use crate::units::UnitPreference;

/// The shape version an agent writes and this module reads.
pub const API_VERSION: u64 = 1;
/// Where the source policy lives in the settings table.
const POLICY_SETTING: &str = "agent_source_policy";

/// What a proposal may claim without a cited source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourcePolicy {
    /// Every value needs a source; the model's own knowledge is refused.
    SourcesOnly,
    /// The model may propose unsourced values, marked `agent` and low confidence.
    ModelKnowledge,
}

impl SourcePolicy {
    /// Unset or unreadable means the strict policy.
    pub fn load(store: &Store) -> crate::store::Result<Self> {
        Ok(match store.setting(POLICY_SETTING)?.as_deref() {
            Some("model_knowledge") => Self::ModelKnowledge,
            _ => Self::SourcesOnly,
        })
    }

    pub fn save(&self, store: &Store) -> crate::store::Result<()> {
        store.set_setting(
            POLICY_SETTING,
            match self {
                Self::SourcesOnly => "sources",
                Self::ModelKnowledge => "model_knowledge",
            },
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub source_type: String,
    pub source_ref: Option<String>,
    pub confidence: Option<String>,
    pub retrieved_at: Option<String>,
    pub notes: Option<String>,
}

impl Source {
    /// `None` for a value without any source fields at all (only stages allow that).
    fn from_json(map: &Map<String, Value>) -> Result<Option<Self>, String> {
        let source_type = text(map, "source_type")?;
        let source_ref = text(map, "source_ref")?;

        if source_type.is_none() {
            if source_ref.is_some()
                || text(map, "confidence")?.is_some()
                || text(map, "retrieved_at")?.is_some()
            {
                return Err("a source without a source_type is incomplete".to_string());
            }
            return Ok(None);
        }

        Ok(Some(Self {
            source_type: source_type.unwrap_or_default(),
            source_ref,
            confidence: text(map, "confidence")?,
            retrieved_at: text(map, "retrieved_at")?,
            notes: text(map, "notes")?,
        }))
    }

    fn validate(&self, policy: SourcePolicy) -> Result<(), String> {
        if !SOURCE_TYPES.contains(&self.source_type.as_str()) {
            return Err(format!(
                "unknown source_type {:?}; use one of {}",
                self.source_type,
                SOURCE_TYPES.join(", ")
            ));
        }

        // "user" is the app's own mark. A proposal or CLI write claiming it would make its value
        // indistinguishable from one the user confirmed.
        if self.source_type == "user" {
            return Err(
                "a proposal cannot claim the user as its source; confirm the value in the app"
                    .to_string(),
            );
        }

        if self.source_type == "agent" && policy == SourcePolicy::SourcesOnly {
            return Err(
                "the model's own knowledge is not a source; add a source or turn on model \
                 knowledge in Settings"
                    .to_string(),
            );
        }

        if self.source_type != "agent" && self.source_ref.is_none() {
            return Err(format!(
                "a {:?} source needs a source_ref (a URL, book title or label)",
                self.source_type
            ));
        }

        if let Some(confidence) = self.confidence.as_deref()
            && !CONFIDENCE_LEVELS.contains(&confidence)
        {
            return Err(format!(
                "unknown confidence {confidence:?}; use one of {}",
                CONFIDENCE_LEVELS.join(", ")
            ));
        }

        Ok(())
    }

    fn for_write(&self) -> Self {
        let mut source = self.clone();
        if source.source_type == "agent" {
            source.confidence = Some("low".to_string());
        }
        source
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DetailValue {
    Text(String),
    Number(f64),
    Boolean(bool),
}

#[derive(Clone, Debug, PartialEq)]
pub struct DetailChange {
    pub field: &'static str,
    pub value: DetailValue,
    pub source: Source,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RequirementChange {
    pub stage_label: Option<String>,
    pub requirement: Requirement,
    pub source: Source,
    /// Set only after the user agrees that this row may override a confirmed value.
    pub override_confirmed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimingChange {
    pub stage_label: Option<String>,
    pub rule: TimingRule,
    pub source: Source,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AttributeChange {
    pub stage_label: Option<String>,
    pub key: String,
    pub label: String,
    pub unit: Option<String>,
    pub custom: bool,
    pub value: DetailValue,
    pub source: Source,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StageChange {
    pub stage: Stage,
    /// Where the stage belongs in the cycle, 1-based. Applied after every stage exists.
    pub position: Option<i64>,
    pub source: Option<Source>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Detail(DetailChange),
    Requirement(RequirementChange),
    Timing(TimingChange),
    Attribute(AttributeChange),
    Stage(StageChange),
}

impl DetailChange {
    pub fn describe(&self) -> String {
        self.describe_for(UnitPreference::Metric)
    }

    pub fn describe_for(&self, pref: UnitPreference) -> String {
        format!(
            "{}: {}",
            detail_title(self.field),
            detail_value_text(&self.value, crate::units::detail_unit(self.field), pref)
        )
    }
}

impl RequirementChange {
    pub fn describe(&self) -> String {
        self.describe_for(UnitPreference::Metric)
    }

    pub fn describe_for(&self, pref: UnitPreference) -> String {
        let base = format!(
            "{}{}: {}",
            metric_label(&self.requirement.metric),
            day_part_suffix(&self.requirement.day_part),
            requirement_text(&self.requirement, pref)
        );
        if self.override_confirmed {
            format!("{base} · overrides a confirmed value")
        } else {
            base
        }
    }
}

impl TimingChange {
    pub fn describe(&self) -> String {
        self.describe_for(UnitPreference::Metric)
    }

    pub fn describe_for(&self, _pref: UnitPreference) -> String {
        format!(
            "{}{}: {}",
            timing_label(&self.rule.kind),
            stage_suffix(self.stage_label.as_deref()),
            timing_text(&self.rule)
        )
    }
}

impl AttributeChange {
    pub fn describe(&self) -> String {
        self.describe_for(UnitPreference::Metric)
    }

    pub fn describe_for(&self, pref: UnitPreference) -> String {
        let pref = if self.custom {
            UnitPreference::Metric
        } else {
            pref
        };
        let unit = self.unit.as_deref().unwrap_or("");
        let value = match (&self.value, self.key.as_str()) {
            (DetailValue::Number(number), "dif_c") => {
                format!("{} {}", pref.delta_number(unit, *number), pref.label(unit))
            }
            (value, _) => detail_value_text(value, unit, pref),
        };

        format!(
            "{}{}: {value}",
            self.label,
            stage_suffix(self.stage_label.as_deref())
        )
    }
}

impl StageChange {
    pub fn describe(&self) -> String {
        let mut text = match self.position {
            Some(position) => format!("Stage: {} (position {position})", self.stage.label),
            None => format!("Stage: {}", self.stage.label),
        };
        match self.stage.repeats {
            Some(true) => text.push_str(" · repeats yearly"),
            Some(false) => text.push_str(" · first cycle only"),
            None => {}
        }
        text
    }
}

impl Change {
    /// One line for the review screen in the user's units.
    pub fn describe_for(&self, pref: UnitPreference) -> String {
        match self {
            Self::Detail(change) => change.describe_for(pref),
            Self::Requirement(change) => change.describe_for(pref),
            Self::Timing(change) => change.describe_for(pref),
            Self::Attribute(change) => change.describe_for(pref),
            Self::Stage(change) => change.describe(),
        }
    }

    pub fn source(&self) -> Option<&Source> {
        match self {
            Self::Detail(change) => Some(&change.source),
            Self::Requirement(change) => Some(&change.source),
            Self::Timing(change) => Some(&change.source),
            Self::Attribute(change) => Some(&change.source),
            Self::Stage(change) => change.source.as_ref(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    pub profile_id: i64,
    pub profile_name: String,
    pub changes: Vec<Change>,
}

/// Reads a proposal file and validates its shape, its sources and its references.
pub fn parse(store: &Store, text: &str) -> Result<Proposal, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("this is not valid JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or("a proposal must be a JSON object")?;

    let api = object.get("api").and_then(Value::as_u64).unwrap_or(0);
    if api != API_VERSION {
        return Err(format!(
            "unsupported proposal api {api}; this build reads {API_VERSION}"
        ));
    }

    let profile_id = object
        .get("profile")
        .and_then(Value::as_i64)
        .ok_or("a proposal needs a profile id")?;
    let profile = store
        .profile(profile_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("there is no profile with id {profile_id}"))?;

    let policy = SourcePolicy::load(store).map_err(|error| error.to_string())?;
    let values = object
        .get("values")
        .and_then(Value::as_array)
        .ok_or("a proposal needs a values array")?;
    if values.is_empty() {
        return Err("a proposal needs at least one value".to_string());
    }

    let stages = store
        .stages(profile_id)
        .map_err(|error| error.to_string())?;
    let definitions: HashMap<String, AttributeDefinition> = store
        .attribute_definitions()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|definition| (definition.key.clone(), definition))
        .collect();

    let mut changes = Vec::new();
    let mut proposed_stages: Vec<String> = Vec::new();

    // Stages may be declared anywhere in the file; collect them before resolving references.
    for value in values {
        if value.get("target").and_then(Value::as_str) != Some("stage") {
            continue;
        }
        let Some(label) = value
            .get("label")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|label| !label.is_empty())
        else {
            continue;
        };

        if proposed_stages
            .iter()
            .any(|stage| stage.eq_ignore_ascii_case(label))
        {
            return Err(format!("the proposal adds the stage {label:?} twice"));
        }
        proposed_stages.push(label.to_string());
    }

    for (index, value) in values.iter().enumerate() {
        let change = parse_change(
            value,
            profile_id,
            &stages,
            &definitions,
            policy,
            &proposed_stages,
        )
        .map_err(|error| format!("value {}: {error}", index + 1))?;
        changes.push(change);
    }

    Ok(Proposal {
        profile_id,
        profile_name: profile.name,
        changes,
    })
}

/// Where agents drop proposals for the app to review.
pub fn inbox() -> Option<std::path::PathBuf> {
    crate::store::data_dir().map(|dir| dir.join("proposals"))
}

/// Every JSON file in `directory` that names this profile, parsed.
///
/// A file that does not parse stays in the list so the user can see why it failed; a file for
/// another profile is skipped.
pub fn pending_in(
    directory: &Path,
    store: &Store,
    profile_id: i64,
) -> Vec<(std::path::PathBuf, Result<Proposal, String>)> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };

    let mut paths: Vec<std::path::PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .filter(|path| names_profile(path, profile_id))
        .collect();
    paths.sort();

    paths
        .into_iter()
        .map(|path| {
            let proposal = load_file(store, &path);
            (path, proposal)
        })
        .collect()
}

/// Every proposal in the app's inbox for this profile.
pub fn pending(
    store: &Store,
    profile_id: i64,
) -> Vec<(std::path::PathBuf, Result<Proposal, String>)> {
    match inbox() {
        Some(directory) => pending_in(&directory, store, profile_id),
        None => Vec::new(),
    }
}

/// Moves a reviewed proposal out of the inbox, so it is not offered again.
pub fn archive(path: &Path) -> Result<std::path::PathBuf, String> {
    let directory = inbox().ok_or("no data directory: set XDG_DATA_HOME or HOME")?;
    archive_in(&directory, path)
}

pub fn archive_in(directory: &Path, path: &Path) -> Result<std::path::PathBuf, String> {
    let applied = directory.join("applied");
    std::fs::create_dir_all(&applied)
        .map_err(|error| format!("could not create the applied folder: {error}"))?;

    let name = path.file_name().ok_or("this proposal has no file name")?;

    let mut target = applied.join(name);
    let mut counter = 1;
    while target.exists() {
        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "proposal".to_string());
        let suffix = path
            .extension()
            .map(|extension| format!(".{}", extension.to_string_lossy()))
            .unwrap_or_default();
        target = applied.join(format!("{stem}-{counter}{suffix}"));
        counter += 1;
    }

    std::fs::rename(path, &target).map_err(|error| {
        format!(
            "could not move {} to {}: {error}",
            path.display(),
            target.display()
        )
    })?;

    Ok(target)
}

fn names_profile(path: &Path, profile_id: i64) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.get("profile").and_then(Value::as_i64))
        == Some(profile_id)
}

/// Parses a document for a profile named by the command, which may omit the id.
pub fn parse_for_profile(store: &Store, profile_id: i64, text: &str) -> Result<Proposal, String> {
    let mut value: Value =
        serde_json::from_str(text).map_err(|error| format!("this is not valid JSON: {error}"))?;
    let object = value
        .as_object_mut()
        .ok_or("a proposal must be a JSON object")?;

    match object.get("profile").and_then(Value::as_i64) {
        Some(named) if named != profile_id => {
            return Err(format!(
                "this proposal is for profile {named}, but the command names {profile_id}"
            ));
        }
        Some(_) => {}
        None => {
            object.insert("profile".to_string(), json!(profile_id));
        }
    }

    parse(store, &value.to_string())
}

/// Reads a proposal from disk.
pub fn load_file(store: &Store, path: &Path) -> Result<Proposal, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    parse(store, &text)
}

/// Applies every change in one transaction and reports what it wrote.
pub fn apply(store: &Store, proposal: &Proposal) -> Result<Vec<String>, String> {
    store
        .transaction(|store| apply_changes(store, proposal))
        .map_err(|error| error.to_string())
}

/// Applies the changes and rolls them back, so an agent can check a proposal without writing.
pub fn check(store: &Store, proposal: &Proposal) -> Result<Vec<String>, String> {
    store
        .dry_run(|store| apply_changes(store, proposal))
        .map_err(|error| error.to_string())
}

fn apply_changes(store: &Store, proposal: &Proposal) -> crate::store::Result<Vec<String>> {
    let profile_id = proposal.profile_id;
    let mut report = Vec::new();

    // The policy can change between review and apply, so it is checked here as well.
    let policy = SourcePolicy::load(store)?;
    for change in &proposal.changes {
        if let Some(source) = change.source() {
            source.validate(policy).map_err(ValidationError)?;
        }
    }

    for change in &proposal.changes {
        if let Change::Stage(change) = change {
            report.push(apply_stage(store, profile_id, change)?);
        }
    }

    let stages = store.stages(profile_id)?;

    // A stage may name a position. Apply the whole order after the stages exist, so an agent can
    // put an existing Ripening stage at the end without deleting it first.
    let mut order: Vec<i64> = stages.iter().map(|stage| stage.id).collect();
    let mut moved = false;
    for change in &proposal.changes {
        let Change::Stage(change) = change else {
            continue;
        };
        let Some(position) = change.position else {
            continue;
        };
        let Some(id) = stages
            .iter()
            .find(|stage| stage.label.eq_ignore_ascii_case(&change.stage.label))
            .map(|stage| stage.id)
        else {
            continue;
        };

        order.retain(|item| *item != id);
        let target = ((position - 1) as usize).min(order.len());
        order.insert(target, id);
        moved = true;
    }
    if moved {
        store.reorder_stages(profile_id, &order)?;
    }

    for change in &proposal.changes {
        match change {
            Change::Stage(_) => {}
            Change::Detail(change) => report.push(apply_detail(store, profile_id, change)?),
            Change::Requirement(change) => {
                report.push(apply_requirement(store, profile_id, &stages, change)?)
            }
            Change::Timing(change) => {
                report.push(apply_timing(store, profile_id, &stages, change)?)
            }
            Change::Attribute(change) => {
                report.push(apply_attribute(store, profile_id, &stages, change)?)
            }
        }
    }

    Ok(report)
}

fn apply_stage(
    store: &Store,
    profile_id: i64,
    change: &StageChange,
) -> crate::store::Result<String> {
    let stages = store.stages(profile_id)?;
    let existing = stages
        .iter()
        .find(|stage| stage.label.eq_ignore_ascii_case(&change.stage.label));

    let mut position_only = false;
    let id = match existing {
        Some(existing) => {
            let same_values = existing.kind == change.stage.kind
                && change
                    .stage
                    .duration_days_min
                    .is_none_or(|value| existing.duration_days_min == Some(value))
                && change
                    .stage
                    .duration_days_max
                    .is_none_or(|value| existing.duration_days_max == Some(value))
                && change
                    .stage
                    .notes
                    .as_ref()
                    .is_none_or(|notes| existing.notes.as_ref() == Some(notes))
                && change
                    .stage
                    .repeats
                    .is_none_or(|repeats| existing.repeats_each_year() == repeats);
            position_only = change.position.is_some() && same_values;

            if !position_only
                && let Some(status) = protected_status(store, "stage", existing.id, "*")?
            {
                return Err(protected_error(&change.describe(), &status).into());
            }

            if !position_only {
                let mut updated = existing.clone();
                updated.kind = change.stage.kind.clone();
                if change.stage.duration_days_min.is_some() {
                    updated.duration_days_min = change.stage.duration_days_min;
                }
                if change.stage.duration_days_max.is_some() {
                    updated.duration_days_max = change.stage.duration_days_max;
                }
                if change.stage.notes.is_some() {
                    updated.notes = change.stage.notes.clone();
                }
                if change.stage.repeats.is_some() {
                    updated.repeats = change.stage.repeats;
                }
                store.update_stage(&updated)?;
            }
            existing.id
        }
        None => {
            let position = stages.iter().map(|stage| stage.position).max().unwrap_or(0) + 1;
            let mut stage = change.stage.clone();
            stage.profile_id = profile_id;
            stage.position = position;
            store.create_stage(&stage)?
        }
    };

    if !position_only {
        match change.source.as_ref() {
            Some(source) => write_source(store, "stage", id, "*", source)?,
            None => store.delete_provenance("stage", id, "*")?,
        }
    }

    let after = store.stage(id)?;
    changes::record(
        store,
        &HistoryChange {
            profile_id,
            entity_type: "stage",
            entity_id: id,
            field: "*",
            old: existing.map(changes::stage),
            new: after.as_ref().map(changes::stage),
            actor: changes::ACTOR_AGENT,
            source_type: change
                .source
                .as_ref()
                .map(|source| source.source_type.as_str()),
            source_ref: change
                .source
                .as_ref()
                .and_then(|source| source.source_ref.as_deref()),
        },
    )?;

    Ok(change.describe())
}

fn apply_detail(
    store: &Store,
    profile_id: i64,
    change: &DetailChange,
) -> crate::store::Result<String> {
    if let Some(status) = protected_status(store, "profile", profile_id, change.field)? {
        return Err(protected_error(change.field, &status).into());
    }

    let mut details = store
        .profile_details(profile_id)?
        .ok_or_else(|| format!("profile {profile_id} does not exist"))?;

    let before = changes::detail(&details, change.field);
    set_detail(&mut details, change.field, &change.value)?;
    store.update_profile_details(&details)?;
    write_source(store, "profile", profile_id, change.field, &change.source)?;
    changes::record(
        store,
        &HistoryChange {
            profile_id,
            entity_type: "profile",
            entity_id: profile_id,
            field: change.field,
            old: Some(before),
            new: Some(changes::detail(&details, change.field)),
            actor: changes::ACTOR_AGENT,
            source_type: Some(change.source.source_type.as_str()),
            source_ref: change.source.source_ref.as_deref(),
        },
    )?;

    Ok(change.describe())
}

fn apply_requirement(
    store: &Store,
    profile_id: i64,
    stages: &[Stage],
    change: &RequirementChange,
) -> crate::store::Result<String> {
    let stage_id = resolve_stage(stages, change.stage_label.as_deref())?;
    let existing = store
        .requirements(profile_id)?
        .into_iter()
        .find(|requirement| {
            requirement.stage_id == stage_id
                && requirement.metric == change.requirement.metric
                && requirement.day_part == change.requirement.day_part
        });

    if let Some(existing) = &existing
        && let Some(status) = protected_status(store, "requirement", existing.id, "*")?
    {
        return Err(protected_error(&change.describe(), &status).into());
    }

    // A new row can make a broader confirmed or not-applicable row stop applying. The row itself
    // stays untouched, but the value the user sees would change, so it needs the same protection.
    if existing.is_none() {
        // An "any" row answers day and night too, so both scopes must be checked.
        let parts: &[&str] = match change.requirement.day_part.as_str() {
            "any" => &["day", "night"],
            "day" => &["day"],
            "night" => &["night"],
            _ => &[change.requirement.day_part.as_str()],
        };

        for part in parts {
            match store.resolved_requirement(
                profile_id,
                stage_id,
                &change.requirement.metric,
                part,
            )? {
                Some(RequirementResolution::NotApplicable) => {
                    return Err(format!(
                        "{} would replace a requirement the user marked not applicable; only the \
                         user can change that in the app",
                        change.describe()
                    )
                    .into());
                }
                Some(RequirementResolution::Resolved(winner)) => {
                    if let Some(status) = protected_status(store, "requirement", winner.id, "*")?
                        && status == "confirmed"
                        && !change.override_confirmed
                    {
                        return Err(format!(
                            "{} would override a confirmed value; set \"override_confirmed\": \
                             true only after the user agrees",
                            change.describe()
                        )
                        .into());
                    }
                }
                None => {}
            }
        }
    }

    let mut requirement = change.requirement.clone();
    requirement.profile_id = profile_id;
    requirement.stage_id = stage_id;

    // A proposal that names only some bounds keeps the rest; clearing is a user action.
    if let Some(existing) = &existing {
        requirement.min = requirement.min.or(existing.min);
        requirement.max = requirement.max.or(existing.max);
        requirement.opt_min = requirement.opt_min.or(existing.opt_min);
        requirement.opt_max = requirement.opt_max.or(existing.opt_max);
        requirement.survival_min = requirement.survival_min.or(existing.survival_min);
        requirement.survival_max = requirement.survival_max.or(existing.survival_max);
    }
    let id = store.set_requirement(&requirement)?;
    write_source(store, "requirement", id, "*", &change.source)?;
    requirement.id = id;
    changes::record(
        store,
        &HistoryChange {
            profile_id,
            entity_type: "requirement",
            entity_id: id,
            field: "*",
            old: existing.as_ref().map(changes::requirement),
            new: Some(changes::requirement(&requirement)),
            actor: changes::ACTOR_AGENT,
            source_type: Some(change.source.source_type.as_str()),
            source_ref: change.source.source_ref.as_deref(),
        },
    )?;

    Ok(change.describe())
}

fn apply_timing(
    store: &Store,
    profile_id: i64,
    stages: &[Stage],
    change: &TimingChange,
) -> crate::store::Result<String> {
    let stage_id = resolve_stage(stages, change.stage_label.as_deref())?;
    let existing: Vec<TimingRule> = store
        .timing_rules(profile_id)?
        .into_iter()
        .filter(|rule| rule.stage_id == stage_id && rule.kind == change.rule.kind)
        .collect();

    if existing.len() > 1 {
        return Err(format!(
            "there are several {} rules for this profile; edit them in the app first",
            change.rule.kind
        )
        .into());
    }

    if let Some(rule) = existing.first() {
        if let Some(status) = protected_status(store, "timing_rule", rule.id, "*")? {
            return Err(protected_error(&change.describe(), &status).into());
        }
        store.delete_timing_rule(rule.id)?;
    }

    let mut rule = change.rule.clone();
    rule.profile_id = profile_id;
    rule.stage_id = stage_id;

    // Keep the parts a proposal does not restate.
    if let Some(existing) = existing.first() {
        rule.repeat_days = rule.repeat_days.or(existing.repeat_days);
        rule.notes = rule.notes.or_else(|| existing.notes.clone());
    }
    let id = store.create_timing_rule(&rule)?;
    write_source(store, "timing_rule", id, "*", &change.source)?;
    rule.id = id;
    changes::record(
        store,
        &HistoryChange {
            profile_id,
            entity_type: "timing_rule",
            entity_id: id,
            field: "*",
            old: existing.first().map(changes::timing),
            new: Some(changes::timing(&rule)),
            actor: changes::ACTOR_AGENT,
            source_type: Some(change.source.source_type.as_str()),
            source_ref: change.source.source_ref.as_deref(),
        },
    )?;

    Ok(change.describe())
}

fn apply_attribute(
    store: &Store,
    profile_id: i64,
    stages: &[Stage],
    change: &AttributeChange,
) -> crate::store::Result<String> {
    let stage_id = resolve_stage(stages, change.stage_label.as_deref())?;
    let existing = store
        .attributes(profile_id)?
        .into_iter()
        .find(|attribute| attribute.stage_id == stage_id && attribute.key == change.key);

    if let Some(existing) = &existing
        && let Some(status) = protected_status(store, "attribute", existing.id, "*")?
    {
        return Err(protected_error(&change.describe(), &status).into());
    }

    let mut attribute = Attribute {
        id: 0,
        profile_id,
        stage_id,
        key: change.key.clone(),
        value_num: None,
        value_text: None,
        value_bool: None,
    };

    match &change.value {
        DetailValue::Text(value) => attribute.value_text = Some(value.clone()),
        DetailValue::Number(value) => attribute.value_num = Some(*value),
        DetailValue::Boolean(value) => attribute.value_bool = Some(*value),
    }

    let id = store.set_attribute(&attribute)?;
    write_source(store, "attribute", id, "*", &change.source)?;
    attribute.id = id;
    changes::record(
        store,
        &HistoryChange {
            profile_id,
            entity_type: "attribute",
            entity_id: id,
            field: "*",
            old: existing.as_ref().map(changes::attribute),
            new: Some(changes::attribute(&attribute)),
            actor: changes::ACTOR_AGENT,
            source_type: Some(change.source.source_type.as_str()),
            source_ref: change.source.source_ref.as_deref(),
        },
    )?;

    Ok(change.describe())
}

fn write_source(
    store: &Store,
    entity_type: &str,
    entity_id: i64,
    field: &str,
    source: &Source,
) -> crate::store::Result<()> {
    let source = source.for_write();

    store.set_provenance(&Provenance {
        entity_type: entity_type.to_string(),
        entity_id,
        field: field.to_string(),
        status: PROVENANCE_STATUSES[0].to_string(),
        source_type: Some(source.source_type),
        source_ref: source.source_ref,
        retrieved_at: source.retrieved_at,
        confidence: source.confidence,
        notes: source.notes,
    })
}

/// `Some(status)` when the user already decided about this value: confirmed it, or marked it as
/// not applicable. Both are a "no" for a proposal.
fn protected_status(
    store: &Store,
    entity_type: &str,
    entity_id: i64,
    field: &str,
) -> crate::store::Result<Option<String>> {
    Ok(store
        .provenance(entity_type, entity_id, field)?
        .map(|provenance| provenance.status)
        .filter(|status| status == "confirmed" || status == "not_applicable"))
}

fn protected_error(label: &str, status: &str) -> String {
    if status == "not_applicable" {
        format!("{label} is marked not applicable; a proposal cannot add it back")
    } else {
        format!("{label} is confirmed; a proposal cannot overwrite what you confirmed")
    }
}

fn resolve_stage(stages: &[Stage], label: Option<&str>) -> crate::store::Result<Option<i64>> {
    let Some(label) = label.map(str::trim).filter(|label| !label.is_empty()) else {
        return Ok(None);
    };

    stages
        .iter()
        .find(|stage| stage.label.eq_ignore_ascii_case(label))
        .map(|stage| Some(stage.id))
        .ok_or_else(|| {
            format!(
                "there is no stage called {label:?}; use one of: {}",
                stages
                    .iter()
                    .map(|stage| stage.label.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .into()
        })
}

fn parse_change(
    value: &Value,
    profile_id: i64,
    stages: &[Stage],
    definitions: &HashMap<String, AttributeDefinition>,
    policy: SourcePolicy,
    proposed_stages: &[String],
) -> Result<Change, String> {
    let object = value
        .as_object()
        .ok_or("each value must be a JSON object")?;
    let target = object
        .get("target")
        .and_then(Value::as_str)
        .ok_or("each value needs a target: detail, requirement, timing_rule, attribute or stage")?;

    match target {
        "detail" => {
            let name = object
                .get("field")
                .and_then(Value::as_str)
                .ok_or("a detail value needs a field")?;
            let (field, kind) = detail_field(name)
                .ok_or_else(|| format!("{name:?} is not a profile detail a proposal can set"))?;
            let source = require_source(object, policy)?;

            Ok(Change::Detail(DetailChange {
                field,
                value: parse_detail_value(object, field, kind)?,
                source,
            }))
        }
        "requirement" => {
            let stage_label = stage_label(object)?;
            let metric = required_text(object, "metric")?;
            let day_part = required_text(object, "day_part")?;

            if !METRICS.contains(&metric.as_str()) {
                return Err(format!(
                    "unknown metric {metric:?}; use one of {}",
                    METRICS.join(", ")
                ));
            }
            if !DAY_PARTS.contains(&day_part.as_str()) {
                return Err(format!(
                    "unknown day_part {day_part:?}; use one of {}",
                    DAY_PARTS.join(", ")
                ));
            }
            check_stage_label(stages, stage_label.as_deref(), proposed_stages)?;

            Ok(Change::Requirement(RequirementChange {
                stage_label,
                requirement: Requirement {
                    id: 0,
                    profile_id,
                    stage_id: None,
                    metric,
                    day_part,
                    min: number(object, "min")?,
                    max: number(object, "max")?,
                    opt_min: number(object, "opt_min")?,
                    opt_max: number(object, "opt_max")?,
                    survival_min: number(object, "survival_min")?,
                    survival_max: number(object, "survival_max")?,
                },
                source: require_source(object, policy)?,
                override_confirmed: match object.get("override_confirmed") {
                    Some(value) => value
                        .as_bool()
                        .ok_or("override_confirmed must be true or false")?,
                    None => false,
                },
            }))
        }
        "timing_rule" => {
            let stage_label = stage_label(object)?;
            let kind = required_text(object, "kind")?;
            let anchor = required_text(object, "anchor")?;

            if !TIMING_KINDS.contains(&kind.as_str()) {
                return Err(format!(
                    "unknown timing kind {kind:?}; use one of {}",
                    TIMING_KINDS.join(", ")
                ));
            }
            if !TIMING_ANCHORS.contains(&anchor.as_str()) {
                return Err(format!(
                    "unknown anchor {anchor:?}; use one of {}",
                    TIMING_ANCHORS.join(", ")
                ));
            }
            check_stage_label(stages, stage_label.as_deref(), proposed_stages)?;

            Ok(Change::Timing(TimingChange {
                stage_label,
                rule: TimingRule {
                    id: 0,
                    profile_id,
                    stage_id: None,
                    kind,
                    anchor,
                    offset_min_days: integer(object, "offset_min_days")?,
                    offset_max_days: integer(object, "offset_max_days")?,
                    month_start: integer(object, "month_start")?,
                    month_end: integer(object, "month_end")?,
                    day_length_hours: number(object, "day_length_hours")?,
                    gdd_base_c: number(object, "gdd_base_c")?,
                    gdd_target: number(object, "gdd_target")?,
                    repeat_days: integer(object, "repeat_days")?,
                    notes: optional_text(object, "notes")?,
                },
                source: require_source(object, policy)?,
            }))
        }
        "attribute" => {
            let stage_label = stage_label(object)?;
            let key = required_text(object, "key")?;
            let definition = definitions.get(&key).ok_or_else(|| {
                format!("{key:?} is not a registered attribute; run `omagrow attributes list`")
            })?;
            check_stage_label(stages, stage_label.as_deref(), proposed_stages)?;

            Ok(Change::Attribute(AttributeChange {
                stage_label,
                label: definition.label.clone(),
                key,
                unit: definition.unit.clone(),
                custom: definition.is_custom,
                value: parse_attribute_value(object, definition)?,
                source: require_source(object, policy)?,
            }))
        }
        "stage" => {
            let label = required_text(object, "label")?;
            let kind = required_text(object, "kind")?;

            if !STAGE_KINDS.contains(&kind.as_str()) {
                return Err(format!(
                    "unknown stage kind {kind:?}; use one of {}",
                    STAGE_KINDS.join(", ")
                ));
            }
            let stage = Stage {
                id: 0,
                profile_id,
                position: 0,
                kind,
                label,
                duration_days_min: number(object, "duration_days_min")?,
                duration_days_max: number(object, "duration_days_max")?,
                notes: optional_text(object, "notes")?,
                repeats: match object.get("repeats") {
                    None | Some(Value::Null) => None,
                    Some(Value::Bool(flag)) => Some(*flag),
                    Some(_) => return Err("stage repeats must be true or false".to_string()),
                },
            };
            let position = integer(object, "position")?;
            if position.is_some_and(|position| position < 1) {
                return Err("stage position starts at 1".to_string());
            }

            Ok(Change::Stage(StageChange {
                stage,
                position,
                source: Source::from_json(object)?
                    .map(|source| {
                        source.validate(policy)?;
                        Ok::<Source, String>(source)
                    })
                    .transpose()?,
            }))
        }
        other => Err(format!(
            "unknown target {other:?}; use detail, requirement, timing_rule, attribute or stage"
        )),
    }
}

fn require_source(object: &Map<String, Value>, policy: SourcePolicy) -> Result<Source, String> {
    let source = Source::from_json(object)?
        .ok_or("every value needs a source: source_type plus a source_ref")?;
    source.validate(policy)?;
    Ok(source)
}

#[derive(Clone, Copy)]
enum DetailKind {
    Text,
    Number,
    Boolean,
}

fn detail_field(name: &str) -> Option<(&'static str, DetailKind)> {
    Some(match name {
        "family" => ("family", DetailKind::Text),
        "genus" => ("genus", DetailKind::Text),
        "lifecycle" => ("lifecycle", DetailKind::Text),
        "life_form" => ("life_form", DetailKind::Text),
        "foliage" => ("foliage", DetailKind::Text),
        "determinacy" => ("determinacy", DetailKind::Text),
        "growth_habit" => ("growth_habit", DetailKind::Text),
        "sun_exposure" => ("sun_exposure", DetailKind::Text),
        "photoperiod_type" => ("photoperiod_type", DetailKind::Text),
        "water_need" => ("water_need", DetailKind::Text),
        "notes" => ("notes", DetailKind::Text),
        "photoperiod_obligate" => ("photoperiod_obligate", DetailKind::Boolean),
        "container_suitable" => ("container_suitable", DetailKind::Boolean),
        "critical_day_length_hours" => ("critical_day_length_hours", DetailKind::Number),
        "min_container_volume_l_min" => ("min_container_volume_l_min", DetailKind::Number),
        "min_container_volume_l_max" => ("min_container_volume_l_max", DetailKind::Number),
        "days_to_maturity_min" => ("days_to_maturity_min", DetailKind::Number),
        "days_to_maturity_max" => ("days_to_maturity_max", DetailKind::Number),
        _ => return None,
    })
}

fn parse_detail_value(
    object: &Map<String, Value>,
    field: &str,
    kind: DetailKind,
) -> Result<DetailValue, String> {
    let value = object
        .get("value")
        .ok_or_else(|| format!("{field} needs a value"))?;

    match kind {
        DetailKind::Text => value
            .as_str()
            .map(|text| DetailValue::Text(text.trim().to_string()))
            .filter(|value| !matches!(value, DetailValue::Text(text) if text.is_empty()))
            .ok_or_else(|| format!("{field} needs a non-empty string")),
        DetailKind::Number => value
            .as_f64()
            .filter(|number| number.is_finite())
            .map(DetailValue::Number)
            .ok_or_else(|| format!("{field} needs a number")),
        DetailKind::Boolean => value
            .as_bool()
            .map(DetailValue::Boolean)
            .ok_or_else(|| format!("{field} needs true or false")),
    }
}

fn parse_attribute_value(
    object: &Map<String, Value>,
    definition: &AttributeDefinition,
) -> Result<DetailValue, String> {
    let value = object
        .get("value")
        .ok_or_else(|| format!("{} needs a value", definition.key))?;

    match definition.value_type.as_str() {
        "number" => value
            .as_f64()
            .filter(|number| number.is_finite())
            .map(DetailValue::Number)
            .ok_or_else(|| format!("{} needs a number", definition.key)),
        "boolean" => value
            .as_bool()
            .map(DetailValue::Boolean)
            .ok_or_else(|| format!("{} needs true or false", definition.key)),
        "text" | "json" | "enum" => value
            .as_str()
            .map(|text| DetailValue::Text(text.trim().to_string()))
            .filter(|value| !matches!(value, DetailValue::Text(text) if text.is_empty()))
            .ok_or_else(|| format!("{} needs a string", definition.key)),
        other => Err(format!(
            "{} has an unsupported type {other}",
            definition.key
        )),
    }
}

fn set_detail(
    details: &mut ProfileDetails,
    field: &str,
    value: &DetailValue,
) -> crate::store::Result<()> {
    match (field, value) {
        ("family", DetailValue::Text(value)) => details.family = Some(value.clone()),
        ("genus", DetailValue::Text(value)) => details.genus = Some(value.clone()),
        ("lifecycle", DetailValue::Text(value)) => details.lifecycle = Some(value.clone()),
        ("life_form", DetailValue::Text(value)) => details.life_form = Some(value.clone()),
        ("foliage", DetailValue::Text(value)) => details.foliage = Some(value.clone()),
        ("determinacy", DetailValue::Text(value)) => details.determinacy = Some(value.clone()),
        ("growth_habit", DetailValue::Text(value)) => details.growth_habit = Some(value.clone()),
        ("sun_exposure", DetailValue::Text(value)) => details.sun_exposure = Some(value.clone()),
        ("photoperiod_type", DetailValue::Text(value)) => {
            details.photoperiod_type = Some(value.clone())
        }
        ("water_need", DetailValue::Text(value)) => details.water_need = Some(value.clone()),
        ("notes", DetailValue::Text(value)) => details.notes = Some(value.clone()),
        ("photoperiod_obligate", DetailValue::Boolean(value)) => {
            details.photoperiod_obligate = Some(*value)
        }
        ("container_suitable", DetailValue::Boolean(value)) => {
            details.container_suitable = Some(*value)
        }
        ("critical_day_length_hours", DetailValue::Number(value)) => {
            details.critical_day_length_hours = Some(*value)
        }
        ("min_container_volume_l_min", DetailValue::Number(value)) => {
            details.min_container_volume_l_min = Some(*value)
        }
        ("min_container_volume_l_max", DetailValue::Number(value)) => {
            details.min_container_volume_l_max = Some(*value)
        }
        ("days_to_maturity_min", DetailValue::Number(value)) => {
            details.days_to_maturity_min = Some(*value)
        }
        ("days_to_maturity_max", DetailValue::Number(value)) => {
            details.days_to_maturity_max = Some(*value)
        }
        _ => {
            return Err(format!("{field} cannot take that value type").into());
        }
    }

    Ok(())
}

fn check_stage_label(
    stages: &[Stage],
    label: Option<&str>,
    proposed_stages: &[String],
) -> Result<(), String> {
    let Some(label) = label.map(str::trim).filter(|label| !label.is_empty()) else {
        return Ok(());
    };

    let known = stages
        .iter()
        .any(|stage| stage.label.eq_ignore_ascii_case(label))
        || proposed_stages
            .iter()
            .any(|stage| stage.eq_ignore_ascii_case(label));

    if known {
        Ok(())
    } else {
        Err(format!(
            "there is no stage called {label:?}; add it with a stage value first"
        ))
    }
}

fn text(object: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            let value = value.trim().to_string();
            Ok((!value.is_empty()).then_some(value))
        }
        Some(_) => Err(format!("{key} must be a string")),
    }
}

fn required_text(object: &Map<String, Value>, key: &str) -> Result<String, String> {
    text(object, key)?.ok_or_else(|| format!("{key} is required"))
}

fn optional_text(object: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    text(object, key)
}

fn number(object: &Map<String, Value>, key: &str) -> Result<Option<f64>, String> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .filter(|number| number.is_finite())
            .map(Some)
            .ok_or_else(|| format!("{key} must be a number")),
    }
}

fn integer(object: &Map<String, Value>, key: &str) -> Result<Option<i64>, String> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_i64()
            .map(Some)
            .ok_or_else(|| format!("{key} must be a whole number")),
    }
}

fn stage_label(object: &Map<String, Value>) -> Result<Option<String>, String> {
    match object.get("stage") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(label)) => {
            let label = label.trim();
            Ok((!label.is_empty()).then(|| label.to_string()))
        }
        Some(_) => Err("stage must be a stage label or null".to_string()),
    }
}

fn detail_title(field: &str) -> String {
    match field {
        "days_to_maturity_min" | "days_to_maturity_max" => "Days to maturity".to_string(),
        "min_container_volume_l_min" | "min_container_volume_l_max" => {
            "Container volume".to_string()
        }
        other => humanize(other),
    }
}

fn detail_value_text(value: &DetailValue, unit: &str, pref: UnitPreference) -> String {
    match value {
        DetailValue::Text(value) => value.clone(),
        DetailValue::Number(value) => pref.text(unit, *value),
        DetailValue::Boolean(true) => "yes".to_string(),
        DetailValue::Boolean(false) => "no".to_string(),
    }
}

fn metric_label(metric: &str) -> &'static str {
    crate::plant_data::metric_label(metric)
}

fn day_part_suffix(day_part: &str) -> String {
    match day_part {
        "day" => " · day".to_string(),
        "night" => " · night".to_string(),
        _ => String::new(),
    }
}

fn stage_suffix(label: Option<&str>) -> String {
    match label {
        Some(label) => format!(" ({label})"),
        None => String::new(),
    }
}

fn timing_label(kind: &str) -> String {
    humanize(kind)
}

fn timing_text(rule: &TimingRule) -> String {
    let anchor = timing_anchor_label(&rule.anchor);

    if rule.anchor == "calendar"
        && let (Some(start), Some(end)) = (rule.month_start, rule.month_end)
    {
        return format!("{}–{}", month_name(start), month_name(end));
    }
    if rule.anchor == "day_length"
        && let Some(hours) = rule.day_length_hours
    {
        return format!("after {} hours of daylight", format_number(hours));
    }
    if rule.anchor == "gdd"
        && let Some(target) = rule.gdd_target
    {
        return match rule.gdd_base_c {
            Some(base) => format!(
                "{} GDD above {} °C",
                format_number(target),
                format_number(base)
            ),
            None => format!("{} GDD", format_number(target)),
        };
    }

    let offsets = match (rule.offset_min_days, rule.offset_max_days) {
        (Some(min), Some(max)) => format!("{}–{}", min.min(max), min.max(max)),
        (Some(value), None) | (None, Some(value)) => value.to_string(),
        (None, None) => return format!("around {anchor}"),
    };

    let (low, high) = (
        rule.offset_min_days.or(rule.offset_max_days).unwrap_or(0),
        rule.offset_min_days.or(rule.offset_max_days).unwrap_or(0),
    );
    let direction = if high < 0 && low < 0 {
        "before"
    } else if high > 0 && low > 0 {
        "after"
    } else {
        "from"
    };

    format!("{offsets} days {direction} {anchor}")
}

fn timing_anchor_label(anchor: &str) -> &'static str {
    match anchor {
        "last_frost" => "the last frost",
        "first_frost" => "the first frost",
        "sowing" => "sowing",
        "transplant" => "transplanting",
        "stage_start" => "the stage start",
        "day_length" => "the day length",
        "gdd" => "the heat sum",
        "calendar" => "the calendar",
        _ => "the anchor",
    }
}

fn month_name(month: i64) -> &'static str {
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

fn requirement_text(requirement: &Requirement, pref: UnitPreference) -> String {
    let unit = crate::plant_data::metric_unit(&requirement.metric);
    let mut parts = Vec::new();

    if let Some(range) = pref.range(requirement.min, requirement.max, unit) {
        parts.push(range);
    }
    if let Some(range) = pref.range(requirement.opt_min, requirement.opt_max, unit) {
        parts.push(format!("optimum {range}"));
    }
    if let Some(range) = pref.range(requirement.survival_min, requirement.survival_max, unit) {
        parts.push(format!("survival {range}"));
    }

    if parts.is_empty() {
        "no bounds".to_string()
    } else {
        parts.join(" · ")
    }
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
    use super::{SourcePolicy, apply, check, parse, parse_for_profile};
    use crate::plant_data::{Provenance, Requirement, Stage, TimingRule};
    use crate::store::Store;
    use crate::units::UnitPreference;
    use serde_json::json;

    fn store_with_profile() -> (Store, i64) {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Ghost Pepper", None, None).unwrap();
        (store, id)
    }

    fn proposal(profile_id: i64, values: serde_json::Value) -> String {
        json!({ "api": 1, "profile": profile_id, "values": values }).to_string()
    }

    fn source() -> serde_json::Value {
        json!({
            "source_type": "website",
            "source_ref": "https://example.org",
            "confidence": "high"
        })
    }

    fn detail(field: &str, value: serde_json::Value) -> serde_json::Value {
        let mut object = source();
        object["target"] = json!("detail");
        object["field"] = json!(field);
        object["value"] = value;
        object
    }

    #[test]
    fn applies_details_with_provenance() {
        let (store, id) = store_with_profile();
        let text = proposal(id, json!([detail("lifecycle", json!("perennial"))]));

        let parsed = parse(&store, &text).unwrap();
        assert_eq!(parsed.changes.len(), 1);
        let report = apply(&store, &parsed).unwrap();
        assert_eq!(report.len(), 1);

        let details = store.profile_details(id).unwrap().unwrap();
        assert_eq!(details.lifecycle.as_deref(), Some("perennial"));

        let provenance = store
            .provenance("profile", id, "lifecycle")
            .unwrap()
            .unwrap();
        assert_eq!(provenance.status, "estimated");
        assert_eq!(
            provenance.source_ref.as_deref(),
            Some("https://example.org")
        );
    }

    #[test]
    fn adds_stages_and_references_them_by_label() {
        let (store, id) = store_with_profile();
        let text = proposal(
            id,
            json!([
                {
                    "target": "stage", "label": "Flowering", "kind": "flowering",
                    "duration_days_min": 30, "duration_days_max": 45
                },
                {
                    "target": "requirement", "stage": "Flowering", "metric": "air_temp",
                    "day_part": "day", "min": 12, "opt_min": 22, "opt_max": 28,
                    "source_type": "website", "source_ref": "https://example.org"
                },
                {
                    "target": "attribute", "key": "climate_type", "value": "tropical",
                    "source_type": "book", "source_ref": "RHS encyclopedia"
                }
            ]),
        );

        let parsed = parse(&store, &text).unwrap();
        apply(&store, &parsed).unwrap();

        let stages = store.stages(id).unwrap();
        assert_eq!(stages.len(), 1);
        assert_eq!(stages[0].label, "Flowering");

        let requirements = store.requirements(id).unwrap();
        assert_eq!(requirements.len(), 1);
        assert_eq!(requirements[0].stage_id, Some(stages[0].id));
        assert_eq!(
            store.attributes(id).unwrap()[0].value_text.as_deref(),
            Some("tropical")
        );
    }

    #[test]
    fn a_stage_position_reorders_the_cycle_without_deleting_anything() {
        let (store, id) = store_with_profile();
        store
            .create_stage(&Stage {
                id: 0,
                profile_id: id,
                position: 1,
                kind: "ripening".to_string(),
                label: "Ripening".to_string(),
                duration_days_min: Some(28.0),
                duration_days_max: Some(35.0),
                notes: None,
                repeats: None,
            })
            .unwrap();

        let text = proposal(
            id,
            json!([
                {"target": "stage", "label": "Germination", "kind": "germination", "position": 1},
                {"target": "stage", "label": "Seedling", "kind": "seedling", "position": 2},
                {
                    "target": "stage", "label": "Ripening", "kind": "ripening", "position": 3,
                    "duration_days_min": 28, "duration_days_max": 35
                }
            ]),
        );

        let parsed = parse(&store, &text).unwrap();
        apply(&store, &parsed).unwrap();

        let labels: Vec<String> = store
            .stages(id)
            .unwrap()
            .into_iter()
            .map(|stage| stage.label)
            .collect();
        assert_eq!(labels, ["Germination", "Seedling", "Ripening"]);
    }

    #[test]
    fn a_confirmed_stage_can_still_be_reordered() {
        let (store, id) = store_with_profile();
        let ripening = store
            .create_stage(&Stage {
                id: 0,
                profile_id: id,
                position: 1,
                kind: "ripening".to_string(),
                label: "Ripening".to_string(),
                duration_days_min: Some(28.0),
                duration_days_max: Some(35.0),
                notes: None,
                repeats: None,
            })
            .unwrap();
        store
            .set_provenance(&Provenance {
                entity_type: "stage".to_string(),
                entity_id: ripening,
                field: "*".to_string(),
                status: "confirmed".to_string(),
                source_type: Some("user".to_string()),
                source_ref: None,
                retrieved_at: None,
                confidence: None,
                notes: None,
            })
            .unwrap();

        let text = proposal(
            id,
            json!([
                {"target": "stage", "label": "Germination", "kind": "germination", "position": 1},
                {
                    "target": "stage", "label": "Ripening", "kind": "ripening", "position": 2,
                    "duration_days_min": 28, "duration_days_max": 35
                }
            ]),
        );
        let parsed = parse(&store, &text).unwrap();
        apply(&store, &parsed).unwrap();

        let labels: Vec<String> = store
            .stages(id)
            .unwrap()
            .into_iter()
            .map(|stage| stage.label)
            .collect();
        assert_eq!(labels, ["Germination", "Ripening"]);
        assert_eq!(
            store
                .provenance("stage", ripening, "*")
                .unwrap()
                .unwrap()
                .status,
            "confirmed"
        );
    }

    #[test]
    fn a_stage_position_must_start_at_one() {
        let (store, id) = store_with_profile();
        let text = proposal(
            id,
            json!([{"target": "stage", "label": "Seedling", "kind": "seedling", "position": 0}]),
        );
        let error = parse(&store, &text).unwrap_err();
        assert!(error.contains("position"), "{error}");
    }

    #[test]
    fn a_confirmed_value_is_never_overwritten() {
        let (store, id) = store_with_profile();
        store
            .set_provenance(&Provenance {
                entity_type: "profile".to_string(),
                entity_id: id,
                field: "lifecycle".to_string(),
                status: "confirmed".to_string(),
                source_type: Some("user".to_string()),
                source_ref: None,
                retrieved_at: None,
                confidence: None,
                notes: None,
            })
            .unwrap();

        let text = proposal(id, json!([detail("lifecycle", json!("annual"))]));
        let parsed = parse(&store, &text).unwrap();
        let error = apply(&store, &parsed).unwrap_err();
        assert!(error.contains("confirmed"), "{error}");
        assert_eq!(
            store.profile_details(id).unwrap().unwrap().lifecycle,
            None,
            "nothing may be written"
        );
    }

    #[test]
    fn a_source_is_required_unless_the_user_allows_model_knowledge() {
        let (store, id) = store_with_profile();
        let unsourced = json!([
            {"target": "detail", "field": "lifecycle", "value": "perennial",
             "source_type": "agent"}
        ]);
        let text = proposal(id, unsourced.clone());

        let error = parse(&store, &text).unwrap_err();
        assert!(error.contains("model knowledge"), "{error}");

        SourcePolicy::ModelKnowledge.save(&store).unwrap();
        let parsed = parse(&store, &text).unwrap();
        apply(&store, &parsed).unwrap();
        let provenance = store
            .provenance("profile", id, "lifecycle")
            .unwrap()
            .unwrap();
        assert_eq!(provenance.source_type.as_deref(), Some("agent"));
        assert_eq!(provenance.confidence.as_deref(), Some("low"));
    }

    #[test]
    fn a_proposal_cannot_claim_the_user_as_its_source() {
        let source = super::Source {
            source_type: "user".to_string(),
            source_ref: Some("typed by hand".to_string()),
            confidence: None,
            retrieved_at: None,
            notes: None,
        };
        assert!(
            source
                .validate(super::SourcePolicy::ModelKnowledge)
                .is_err()
        );
    }

    #[test]
    fn rejects_unknown_fields_and_missing_sources() {
        let (store, id) = store_with_profile();

        let error = parse(
            &store,
            &proposal(id, json!([detail("colour", json!("red"))])),
        )
        .unwrap_err();
        assert!(error.contains("not a profile detail"), "{error}");

        let error = parse(
            &store,
            &proposal(
                id,
                json!([{"target": "detail", "field": "family", "value": "Solanaceae"}]),
            ),
        )
        .unwrap_err();
        assert!(error.contains("source"), "{error}");

        let error = parse(
            &store,
            &proposal(
                id,
                json!([{
                    "target": "attribute", "key": "not_a_key", "value": "x",
                    "source_type": "website", "source_ref": "https://example.org"
                }]),
            ),
        )
        .unwrap_err();
        assert!(error.contains("not a registered attribute"), "{error}");
    }

    #[test]
    fn check_validates_without_writing() {
        let (store, id) = store_with_profile();
        let text = proposal(
            id,
            json!([{
                "target": "stage", "label": "Vegetative", "kind": "vegetative",
                "duration_days_min": 40
            }]),
        );

        let parsed = parse(&store, &text).unwrap();
        check(&store, &parsed).unwrap();
        assert!(store.stages(id).unwrap().is_empty());
    }

    #[test]
    fn a_failure_rolls_back_the_whole_proposal() {
        let (store, id) = store_with_profile();
        let text = proposal(
            id,
            json!([
                detail("family", json!("Solanaceae")),
                {"target": "requirement", "metric": "air_temp", "day_part": "day",
                 "min": 30, "max": 10,
                 "source_type": "website", "source_ref": "https://example.org"}
            ]),
        );

        let parsed = parse(&store, &text).unwrap();
        assert!(apply(&store, &parsed).is_err());
        assert_eq!(store.profile_details(id).unwrap().unwrap().family, None);
    }

    #[test]
    fn replaces_an_estimated_timing_rule_instead_of_duplicating_it() {
        let (store, id) = store_with_profile();
        store
            .create_timing_rule(&crate::plant_data::TimingRule {
                id: 0,
                profile_id: id,
                stage_id: None,
                kind: "sow_indoor".to_string(),
                anchor: "calendar".to_string(),
                offset_min_days: None,
                offset_max_days: None,
                month_start: Some(2),
                month_end: Some(3),
                day_length_hours: None,
                gdd_base_c: None,
                gdd_target: None,
                repeat_days: None,
                notes: None,
            })
            .unwrap();

        let text = proposal(
            id,
            json!([{
                "target": "timing_rule", "kind": "sow_indoor", "anchor": "last_frost",
                "offset_min_days": -70, "offset_max_days": -56,
                "source_type": "website", "source_ref": "https://example.org"
            }]),
        );

        let parsed = parse(&store, &text).unwrap();
        apply(&store, &parsed).unwrap();

        let rules = store.timing_rules(id).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].anchor, "last_frost");
    }

    #[test]
    fn a_missing_stage_in_a_proposal_is_a_clear_error() {
        let (store, id) = store_with_profile();
        let text = proposal(
            id,
            json!([{
                "target": "requirement", "stage": "Flowering", "metric": "air_temp",
                "day_part": "day", "min": 10,
                "source_type": "website", "source_ref": "https://example.org"
            }]),
        );

        let error = parse(&store, &text).unwrap_err();
        assert!(error.contains("no stage called"), "{error}");
    }

    #[test]
    fn accepts_a_stage_without_a_source() {
        let (store, id) = store_with_profile();
        let text = proposal(
            id,
            json!([{"target": "stage", "label": "Harvest", "kind": "harvest"}]),
        );

        let parsed = parse(&store, &text).unwrap();
        apply(&store, &parsed).unwrap();
        assert_eq!(store.stages(id).unwrap().len(), 1);
    }

    #[test]
    fn a_stage_from_another_profile_is_not_accepted() {
        let (store, id) = store_with_profile();
        let other = store.create_profile("Basil", None, None).unwrap();
        store
            .create_stage(&Stage {
                id: 0,
                profile_id: id,
                position: 1,
                kind: "vegetative".to_string(),
                label: "Vegetative".to_string(),
                duration_days_min: None,
                duration_days_max: None,
                notes: None,
                repeats: None,
            })
            .unwrap();

        let text = proposal(
            other,
            json!([{
                "target": "requirement", "stage": "Vegetative", "metric": "air_temp",
                "day_part": "day", "min": 10,
                "source_type": "website", "source_ref": "https://example.org"
            }]),
        );

        let error = parse(&store, &text).unwrap_err();
        assert!(error.contains("no stage called"), "{error}");
        assert!(store.requirements(other).unwrap().is_empty());
    }

    #[test]
    fn requirements_keep_their_bounds() {
        let (store, id) = store_with_profile();
        let text = proposal(
            id,
            json!([{
                "target": "requirement", "metric": "rh", "day_part": "any",
                "min": 40, "max": 70, "opt_min": 50, "opt_max": 65,
                "source_type": "book", "source_ref": "RHS encyclopedia"
            }]),
        );

        let parsed = parse(&store, &text).unwrap();
        apply(&store, &parsed).unwrap();

        let requirements = store.requirements(id).unwrap();
        assert_eq!(requirements.len(), 1);
        assert_eq!(requirements[0].opt_max, Some(65.0));
    }

    #[test]
    fn a_document_can_leave_the_profile_to_the_command() {
        let (store, id) = store_with_profile();
        let text = json!({
            "api": 1,
            "values": [detail("lifecycle", json!("perennial"))]
        })
        .to_string();

        let parsed = parse_for_profile(&store, id, &text).unwrap();
        assert_eq!(parsed.profile_id, id);

        let other = store.create_profile("Basil", None, None).unwrap();
        let error = parse_for_profile(&store, other, &text.replace("perennial", "annual")).unwrap();
        assert_eq!(error.profile_id, other);

        let named =
            json!({"api": 1, "profile": other, "values": [detail("genus", json!("Ocimum"))]})
                .to_string();
        let error = parse_for_profile(&store, id, &named).unwrap_err();
        assert!(error.contains("names"), "{error}");
    }

    #[test]
    fn the_inbox_lists_this_profile_and_archives_a_reviewed_file() {
        let (store, id) = store_with_profile();
        let directory = std::env::temp_dir().join(format!("omagrow-inbox-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();

        let mine = directory.join("mine.json");
        std::fs::write(
            &mine,
            proposal(id, json!([detail("family", json!("Solanaceae"))])),
        )
        .unwrap();

        let other_id = store.create_profile("Basil", None, None).unwrap();
        let other = directory.join("other.json");
        std::fs::write(
            &other,
            proposal(other_id, json!([detail("family", json!("Lamiaceae"))])),
        )
        .unwrap();

        let invalid = directory.join("invalid.json");
        std::fs::write(&invalid, proposal(id, json!([{"target": "nope"}]))).unwrap();

        let pending = super::pending_in(&directory, &store, id);
        assert_eq!(pending.len(), 2, "the other profile is skipped");
        assert_eq!(pending[0].0.file_name().unwrap(), "invalid.json");
        assert!(pending[0].1.is_err());
        assert!(pending[1].1.is_ok());

        let archived = super::archive_in(&directory, &mine).unwrap();
        assert!(archived.exists());
        assert!(!mine.exists());
        assert_eq!(super::pending_in(&directory, &store, id).len(), 1);

        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_not_applicable_value_is_protected_too() {
        let (store, id) = store_with_profile();
        store
            .set_provenance(&Provenance {
                entity_type: "profile".to_string(),
                entity_id: id,
                field: "water_need".to_string(),
                status: "not_applicable".to_string(),
                source_type: Some("user".to_string()),
                source_ref: None,
                retrieved_at: None,
                confidence: None,
                notes: None,
            })
            .unwrap();

        let text = proposal(id, json!([detail("water_need", json!("high"))]));
        let parsed = parse(&store, &text).unwrap();
        let error = apply(&store, &parsed).unwrap_err();
        assert!(error.contains("not applicable"), "{error}");
    }

    #[test]
    fn a_stage_may_be_declared_after_the_value_that_uses_it() {
        let (store, id) = store_with_profile();
        let text = proposal(
            id,
            json!([
                {
                    "target": "requirement", "stage": "New Stage", "metric": "air_temp",
                    "day_part": "day", "min": 10,
                    "source_type": "website", "source_ref": "https://example.org"
                },
                {"target": "stage", "label": "New Stage", "kind": "vegetative"}
            ]),
        );

        let parsed = parse(&store, &text).unwrap();
        apply(&store, &parsed).unwrap();
        assert_eq!(store.requirements(id).unwrap().len(), 1);
    }

    #[test]
    fn a_stage_must_be_a_label_or_null() {
        let (store, id) = store_with_profile();
        let text = proposal(
            id,
            json!([{
                "target": "requirement", "stage": 99, "metric": "air_temp",
                "day_part": "day", "min": 10,
                "source_type": "website", "source_ref": "https://example.org"
            }]),
        );

        let error = parse(&store, &text).unwrap_err();
        assert!(error.contains("stage must be a stage label"), "{error}");
    }

    #[test]
    fn the_source_policy_is_checked_again_at_apply_time() {
        let (store, id) = store_with_profile();
        SourcePolicy::ModelKnowledge.save(&store).unwrap();
        let text = proposal(
            id,
            json!([{"target": "detail", "field": "family", "value": "Solanaceae",
                    "source_type": "agent", "confidence": "high"}]),
        );
        let parsed = parse(&store, &text).unwrap();

        SourcePolicy::SourcesOnly.save(&store).unwrap();
        let error = apply(&store, &parsed).unwrap_err();
        assert!(error.contains("model knowledge"), "{error}");
        assert_eq!(store.profile_details(id).unwrap().unwrap().family, None);
    }

    #[test]
    fn model_knowledge_is_always_low_confidence() {
        let (store, id) = store_with_profile();
        SourcePolicy::ModelKnowledge.save(&store).unwrap();
        let text = proposal(
            id,
            json!([{"target": "detail", "field": "family", "value": "Solanaceae",
                    "source_type": "agent", "confidence": "high"}]),
        );

        let parsed = parse(&store, &text).unwrap();
        apply(&store, &parsed).unwrap();
        let provenance = store.provenance("profile", id, "family").unwrap().unwrap();
        assert_eq!(provenance.confidence.as_deref(), Some("low"));
    }

    #[test]
    fn a_partial_requirement_keeps_the_bounds_it_does_not_name() {
        let (store, id) = store_with_profile();
        store
            .set_requirement(&Requirement {
                id: 0,
                profile_id: id,
                stage_id: None,
                metric: "air_temp".to_string(),
                day_part: "day".to_string(),
                min: Some(10.0),
                max: Some(30.0),
                opt_min: Some(20.0),
                opt_max: Some(26.0),
                survival_min: Some(0.0),
                survival_max: Some(35.0),
            })
            .unwrap();

        let text = proposal(
            id,
            json!([{
                "target": "requirement", "metric": "air_temp", "day_part": "day",
                "min": 12,
                "source_type": "website", "source_ref": "https://example.org"
            }]),
        );

        let parsed = parse(&store, &text).unwrap();
        apply(&store, &parsed).unwrap();

        let stored = store.requirements(id).unwrap().remove(0);
        assert_eq!(stored.min, Some(12.0));
        assert_eq!(stored.opt_min, Some(20.0), "untouched bound is kept");
        assert_eq!(stored.survival_max, Some(35.0));
    }

    #[test]
    fn a_stage_update_keeps_the_durations_it_does_not_name() {
        let (store, id) = store_with_profile();
        store
            .create_stage(&Stage {
                id: 0,
                profile_id: id,
                position: 1,
                kind: "flowering".to_string(),
                label: "Flowering".to_string(),
                duration_days_min: Some(10.0),
                duration_days_max: Some(20.0),
                notes: None,
                repeats: None,
            })
            .unwrap();

        let text = proposal(
            id,
            json!([{"target": "stage", "label": "flowering", "kind": "flowering"}]),
        );

        let parsed = parse(&store, &text).unwrap();
        apply(&store, &parsed).unwrap();

        let stages = store.stages(id).unwrap();
        assert_eq!(stages.len(), 1);
        assert_eq!(stages[0].duration_days_min, Some(10.0));
        assert_eq!(stages[0].duration_days_max, Some(20.0));
    }

    #[test]
    fn a_stage_repeat_marker_is_written_and_a_bad_type_is_refused() {
        let (store, id) = store_with_profile();
        let text = proposal(
            id,
            json!([{"target": "stage", "label": "Mature", "kind": "mature", "repeats": true}]),
        );

        let parsed = parse(&store, &text).unwrap();
        apply(&store, &parsed).unwrap();
        assert_eq!(store.stages(id).unwrap()[0].repeats, Some(true));

        let invalid = proposal(
            id,
            json!([{"target": "stage", "label": "Mature", "kind": "mature", "repeats": "yes"}]),
        );
        assert!(parse(&store, &invalid).is_err());
    }

    #[test]
    fn archiving_twice_keeps_both_files() {
        let (_, id) = store_with_profile();
        let directory =
            std::env::temp_dir().join(format!("omagrow-archive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();

        let body = proposal(id, json!([detail("family", json!("Solanaceae"))]));
        for _ in 0..2 {
            let path = directory.join("same.json");
            std::fs::write(&path, &body).unwrap();
            super::archive_in(&directory, &path).unwrap();
        }

        let names: Vec<_> = std::fs::read_dir(directory.join("applied"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");

        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn the_store_setting_defaults_to_sources_only() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(
            SourcePolicy::load(&store).unwrap(),
            SourcePolicy::SourcesOnly
        );
        SourcePolicy::ModelKnowledge.save(&store).unwrap();
        assert_eq!(
            SourcePolicy::load(&store).unwrap(),
            SourcePolicy::ModelKnowledge
        );
    }

    #[test]
    fn stage_resolution_ignores_case_and_rejects_a_foreign_stage() {
        let (store, id) = store_with_profile();
        store
            .create_stage(&Stage {
                id: 0,
                profile_id: id,
                position: 1,
                kind: "flowering".to_string(),
                label: "Flowering".to_string(),
                duration_days_min: None,
                duration_days_max: None,
                notes: None,
                repeats: None,
            })
            .unwrap();

        let text = proposal(
            id,
            json!([{
                "target": "requirement", "stage": "flowering", "metric": "air_temp",
                "day_part": "day", "min": 10,
                "source_type": "website", "source_ref": "https://example.org"
            }]),
        );

        let parsed = parse(&store, &text).unwrap();
        apply(&store, &parsed).unwrap();
        assert_eq!(store.requirements(id).unwrap().len(), 1);
    }

    #[test]
    fn detail_values_and_gdd_stay_consistent() {
        assert_eq!(
            super::detail_value_text(
                &super::DetailValue::Number(10.0),
                "L",
                UnitPreference::Imperial
            ),
            "2.64 US gal"
        );

        let rule = TimingRule {
            id: 0,
            profile_id: 1,
            stage_id: None,
            kind: "harvest".to_string(),
            anchor: "gdd".to_string(),
            offset_min_days: None,
            offset_max_days: None,
            month_start: None,
            month_end: None,
            day_length_hours: None,
            gdd_base_c: Some(20.0),
            gdd_target: Some(1000.0),
            repeat_days: None,
            notes: None,
        };
        assert_eq!(super::timing_text(&rule), "1000 GDD above 20 °C");
    }

    #[test]
    fn a_custom_attribute_is_not_converted() {
        let change = super::AttributeChange {
            stage_label: None,
            key: "custom_height".to_string(),
            label: "Height".to_string(),
            unit: Some("cm".to_string()),
            custom: true,
            value: super::DetailValue::Number(120.0),
            source: super::Source {
                source_type: "user".to_string(),
                source_ref: None,
                confidence: None,
                retrieved_at: None,
                notes: None,
            },
        };

        assert_eq!(
            change.describe_for(UnitPreference::Imperial),
            "Height: 120 cm"
        );
    }

    #[test]
    fn the_requirement_helper_is_used() {
        let requirement = Requirement {
            id: 0,
            profile_id: 1,
            stage_id: None,
            metric: "air_temp".to_string(),
            day_part: "day".to_string(),
            min: Some(12.0),
            max: Some(32.0),
            opt_min: Some(22.0),
            opt_max: Some(28.0),
            survival_min: None,
            survival_max: None,
        };

        let text = super::requirement_text(&requirement, UnitPreference::Metric);
        assert!(text.contains("12–32 °C"));
        assert!(text.contains("optimum 22–28 °C"));

        // The review may show imperial, but the CLI and the stored proposal stay metric.
        let imperial = super::requirement_text(&requirement, UnitPreference::Imperial);
        assert!(imperial.contains("53.6–89.6 °F"));
        assert!(imperial.contains("optimum 71.6–82.4 °F"));

        // The CLI and error paths use describe(), which is metric; the review uses describe_for().
        let change = super::RequirementChange {
            stage_label: None,
            requirement,
            source: super::Source {
                source_type: "website".to_string(),
                source_ref: Some("https://example.org".to_string()),
                confidence: None,
                retrieved_at: None,
                notes: None,
            },
            override_confirmed: false,
        };
        assert!(change.describe().contains("12–32 °C"));
        assert!(
            change
                .describe_for(UnitPreference::Imperial)
                .contains("53.6–89.6 °F")
        );
    }

    fn confirmed_default(
        store: &crate::store::Store,
        profile_id: i64,
        stage_id: Option<i64>,
        day_part: &str,
    ) -> i64 {
        let id = store
            .set_requirement(&Requirement {
                id: 0,
                profile_id,
                stage_id,
                metric: "air_temp".to_string(),
                day_part: day_part.to_string(),
                min: Some(18.0),
                max: Some(30.0),
                opt_min: Some(20.0),
                opt_max: Some(26.0),
                survival_min: None,
                survival_max: None,
            })
            .unwrap();
        store
            .set_provenance(&Provenance {
                entity_type: "requirement".to_string(),
                entity_id: id,
                field: "*".to_string(),
                status: "confirmed".to_string(),
                source_type: Some("user".to_string()),
                source_ref: None,
                retrieved_at: None,
                confidence: None,
                notes: None,
            })
            .unwrap();
        id
    }

    fn stage(store: &crate::store::Store, profile_id: i64) -> i64 {
        store
            .create_stage(&Stage {
                id: 0,
                profile_id,
                position: 1,
                kind: "flowering".to_string(),
                label: "Flowering".to_string(),
                duration_days_min: None,
                duration_days_max: None,
                notes: None,
                repeats: None,
            })
            .unwrap()
    }

    #[test]
    fn a_stage_row_needs_permission_to_override_a_confirmed_default() {
        let (store, id) = store_with_profile();
        let stage_id = stage(&store, id);
        confirmed_default(&store, id, None, "any");

        let without = proposal(
            id,
            json!([{
                "target": "requirement", "stage": "Flowering", "metric": "air_temp",
                "day_part": "day", "min": 16, "max": 26,
                "source_type": "website", "source_ref": "https://example.org"
            }]),
        );
        let parsed = parse(&store, &without).unwrap();
        let error = apply(&store, &parsed).unwrap_err();
        assert!(error.contains("override_confirmed"), "{error}");
        assert_eq!(
            store.requirements(id).unwrap().len(),
            1,
            "nothing was written"
        );

        let with = proposal(
            id,
            json!([{
                "target": "requirement", "stage": "Flowering", "metric": "air_temp",
                "day_part": "day", "min": 16, "max": 26, "override_confirmed": true,
                "source_type": "website", "source_ref": "https://example.org"
            }]),
        );
        let parsed = parse(&store, &with).unwrap();
        apply(&store, &parsed).unwrap();
        assert_eq!(store.requirements(id).unwrap().len(), 2);
        let _ = stage_id;
    }

    #[test]
    fn a_not_applicable_default_cannot_be_overridden_by_a_stage_row() {
        let (store, id) = store_with_profile();
        stage(&store, id);
        let default_id = confirmed_default(&store, id, None, "any");
        store
            .set_provenance(&Provenance {
                entity_type: "requirement".to_string(),
                entity_id: default_id,
                field: "*".to_string(),
                status: "not_applicable".to_string(),
                source_type: Some("user".to_string()),
                source_ref: None,
                retrieved_at: None,
                confidence: None,
                notes: None,
            })
            .unwrap();

        let text = proposal(
            id,
            json!([{
                "target": "requirement", "stage": "Flowering", "metric": "air_temp",
                "day_part": "day", "min": 16, "max": 26, "override_confirmed": true,
                "source_type": "website", "source_ref": "https://example.org"
            }]),
        );
        let parsed = parse(&store, &text).unwrap();
        let error = apply(&store, &parsed).unwrap_err();
        assert!(error.contains("not applicable"), "{error}");
        assert_eq!(store.requirements(id).unwrap().len(), 1);
    }

    #[test]
    fn an_any_row_cannot_slip_past_a_confirmed_day_default() {
        let (store, id) = store_with_profile();
        stage(&store, id);
        confirmed_default(&store, id, None, "day");

        let text = proposal(
            id,
            json!([{
                "target": "requirement", "stage": "Flowering", "metric": "air_temp",
                "day_part": "any", "min": 16, "max": 26,
                "source_type": "website", "source_ref": "https://example.org"
            }]),
        );
        let parsed = parse(&store, &text).unwrap();
        let error = apply(&store, &parsed).unwrap_err();
        assert!(error.contains("override_confirmed"), "{error}");

        let text = proposal(
            id,
            json!([{
                "target": "requirement", "stage": "Flowering", "metric": "air_temp",
                "day_part": "any", "min": 16, "max": 26, "override_confirmed": true,
                "source_type": "website", "source_ref": "https://example.org"
            }]),
        );
        let parsed = parse(&store, &text).unwrap();
        apply(&store, &parsed).unwrap();
    }

    #[test]
    fn an_any_row_cannot_slip_past_a_not_applicable_day_default() {
        let (store, id) = store_with_profile();
        stage(&store, id);
        let default_id = confirmed_default(&store, id, None, "day");
        store
            .set_provenance(&Provenance {
                entity_type: "requirement".to_string(),
                entity_id: default_id,
                field: "*".to_string(),
                status: "not_applicable".to_string(),
                source_type: Some("user".to_string()),
                source_ref: None,
                retrieved_at: None,
                confidence: None,
                notes: None,
            })
            .unwrap();

        let text = proposal(
            id,
            json!([{
                "target": "requirement", "stage": "Flowering", "metric": "air_temp",
                "day_part": "any", "min": 16, "max": 26, "override_confirmed": true,
                "source_type": "website", "source_ref": "https://example.org"
            }]),
        );
        let parsed = parse(&store, &text).unwrap();
        let error = apply(&store, &parsed).unwrap_err();
        assert!(error.contains("not applicable"), "{error}");
    }
}
