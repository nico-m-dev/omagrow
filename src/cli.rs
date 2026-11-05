//! Command-line surface for agents.
//!
//! Every command runs before GTK starts, so it works without a display. Profile output is
//! compact JSON so an agent does not have to parse prose. `agent guide` prints the contract that
//! ships as `assets/agent/SKILL.md`; that file is the single source for both.

use std::collections::HashMap;
use std::process::exit;

use serde_json::{Map, Value, json};

use crate::changes;
use crate::plant_data::{
    APPLIES_TO, Attribute, AttributeDefinition, CONFIDENCE_LEVELS, DAY_PARTS, DETERMINACY, FOLIAGE,
    GROWTH_HABITS, LIFE_FORMS, LIFECYCLES, METRICS, PHOTOPERIOD_TYPES, PROVENANCE_STATUSES,
    Provenance, Requirement, SOURCE_TYPES, STAGE_KINDS, SUN_EXPOSURES, Stage, TIMING_ANCHORS,
    TIMING_KINDS, TimingRule, VALUE_TYPES, WATER_NEEDS, metric_unit,
};
use crate::proposal;
use crate::store::Store;

const GUIDE: &str = include_str!("../assets/agent/SKILL.md");
const SHOW_USAGE: &str = "Usage: omagrow profile show <id>\n\n\
     Prints one profile as compact JSON: details, stages, requirements, timing rules, attributes\n\
     and provenance.";

/// Runs a command when the arguments name one, otherwise returns so the app can start.
pub fn run_if_command(args: &[String]) {
    // GApplication passes this flag to D-Bus activation; it belongs to the window.
    if args
        .iter()
        .any(|argument| argument == "--gapplication-service")
    {
        return;
    }

    let Some(outcome) = dispatch(args) else {
        return;
    };

    match outcome {
        Ok(output) => {
            write_stdout(&output);
            exit(0);
        }
        Err(error) => {
            eprintln!("{}", error.message);
            exit(error.code);
        }
    }
}

/// Writes without panicking when the reader stops early (`omagrow … | head`).
fn write_stdout(text: &str) {
    let mut stdout = std::io::stdout().lock();
    let mut written = std::io::Write::write_all(&mut stdout, text.as_bytes());

    if written.is_ok() && !text.ends_with('\n') {
        written = std::io::Write::write_all(&mut stdout, b"\n");
    }

    if let Err(error) = written
        && error.kind() != std::io::ErrorKind::BrokenPipe
    {
        eprintln!("could not write output: {error}");
        exit(1);
    }
}

#[derive(Debug)]
struct CliError {
    message: String,
    code: i32,
}

type CliResult<T> = Result<T, CliError>;

impl From<String> for CliError {
    fn from(message: String) -> Self {
        Self { message, code: 1 }
    }
}

impl From<Box<dyn std::error::Error>> for CliError {
    fn from(error: Box<dyn std::error::Error>) -> Self {
        Self {
            message: format!("database error: {error}"),
            code: 1,
        }
    }
}

fn usage(message: &str) -> CliError {
    CliError {
        message: format!("{message}\n\nRun `omagrow --help` for the available commands."),
        code: 2,
    }
}

/// `None` means "no command"; the caller starts the window.
fn dispatch(args: &[String]) -> Option<CliResult<String>> {
    let command = args.first()?.as_str();

    Some(match command {
        "agent" => match args.get(1).map(String::as_str) {
            Some("guide") if args.len() == 2 => Ok(guide_text().to_string()),
            _ => Err(usage("Usage: omagrow agent guide")),
        },
        "profile" => match args.get(1).map(String::as_str) {
            Some("list") if args.len() == 2 => list_profiles(),
            Some("show")
                if args
                    .get(2)
                    .is_some_and(|argument| argument == "--help" || argument == "-h") =>
            {
                Ok(SHOW_USAGE.to_string())
            }
            Some("show") => match args.get(2) {
                Some(id) if args.len() == 3 => match id.parse::<i64>() {
                    Ok(id) => show_profile(id),
                    Err(_) => Err(usage("The profile id must be a number.")),
                },
                _ => Err(usage("Usage: omagrow profile show <id>")),
            },
            Some("create") => create_profile(&args[2..]),
            Some("update") => match args.get(2) {
                Some(id) => update_profile(id, &args[3..]),
                None => Err(usage(
                    "Usage: omagrow profile update <id> [--name NAME] [--species NAME]                      [--cultivar NAME]",
                )),
            },
            Some("copy") => match args.get(2) {
                Some(id) => copy_profile(id, &args[3..]),
                None => Err(usage(
                    "Usage: omagrow profile copy <id> [--name NAME] [--species NAME]                      [--cultivar NAME]",
                )),
            },
            _ => Err(usage(
                "Usage: omagrow profile list | show <id> | create | update <id> | copy <id>",
            )),
        },
        "attributes" => match args.get(1).map(String::as_str) {
            Some("list") if args.len() == 2 => list_attributes(),
            _ => Err(usage("Usage: omagrow attributes list")),
        },
        "import" => match (args.get(1), args.get(2)) {
            (Some(path), None) => import(path, false),
            (Some(path), Some(flag)) if args.len() == 3 && flag == "--check" => import(path, true),
            _ => Err(usage("Usage: omagrow import <file> [--check]")),
        },
        "set" => match (args.get(1), args.get(2)) {
            (Some(id), None) => set(id, false),
            (Some(id), Some(flag)) if args.len() == 3 && flag == "--check" => set(id, true),
            _ => Err(usage("Usage: omagrow set <id> [--check]  (JSON on stdin)")),
        },
        "help" | "--help" | "-h" if args.len() == 1 => Ok(usage_text().to_string()),
        "version" | "--version" | "-v" if args.len() == 1 => {
            Ok(format!("omagrow {}", env!("CARGO_PKG_VERSION")))
        }
        "help" | "--help" | "-h" => Err(usage("Usage: omagrow --help")),
        "version" | "--version" | "-v" => Err(usage("Usage: omagrow --version")),
        _ => Err(usage(&format!("Unknown command: {command}"))),
    })
}

fn list_profiles() -> CliResult<String> {
    let store = open_store()?;
    profiles_json(&store)
}

fn show_profile(id: i64) -> CliResult<String> {
    let store = open_store()?;
    profile_json(&store, id)
}

#[derive(Default)]
struct IdentityArgs {
    name: Option<String>,
    species: Option<String>,
    cultivar: Option<String>,
}

fn parse_identity_flags(args: &[String]) -> CliResult<IdentityArgs> {
    let mut identity = IdentityArgs::default();
    let mut index = 0;

    while index < args.len() {
        let flag = args[index].as_str();
        if !matches!(flag, "--name" | "--species" | "--cultivar") {
            return Err(usage(&format!("Unknown option: {flag}")));
        }

        let Some(value) = args.get(index + 1) else {
            return Err(usage(&format!("{flag} needs a value")));
        };
        if value.starts_with("--") {
            return Err(usage(&format!("{flag} needs a value")));
        }

        let slot = match flag {
            "--name" => &mut identity.name,
            "--species" => &mut identity.species,
            _ => &mut identity.cultivar,
        };
        if slot.is_some() {
            return Err(usage(&format!("{flag} was given twice")));
        }
        *slot = Some(value.clone());
        index += 2;
    }

    Ok(identity)
}

fn create_profile(args: &[String]) -> CliResult<String> {
    let identity = parse_identity_flags(args)?;
    let name = identity.name.as_deref().ok_or_else(|| {
        usage("Usage: omagrow profile create --name NAME [--species NAME] [--cultivar NAME]")
    })?;
    let store = open_store_for_writing()?;
    let id = store.create_profile(
        name,
        identity.species.as_deref(),
        identity.cultivar.as_deref(),
    )?;
    let created = store
        .profile(id)?
        .ok_or_else(|| "the new profile disappeared".to_string())?;

    envelope([
        ("profile".to_string(), json!(created.id)),
        ("profile_name".to_string(), json!(created.name)),
        ("created".to_string(), json!(true)),
    ])
}

fn update_profile(id: &str, args: &[String]) -> CliResult<String> {
    let id: i64 = id
        .parse()
        .map_err(|_| usage("The profile id must be a number."))?;
    let identity = parse_identity_flags(args)?;
    if identity.name.is_none() && identity.species.is_none() && identity.cultivar.is_none() {
        return Err(usage(
            "Give at least one of --name, --species or --cultivar",
        ));
    }

    let store = open_store_for_writing()?;
    let before = store
        .profile(id)?
        .ok_or_else(|| format!("there is no profile with id {id}"))?;
    let name = identity.name.unwrap_or_else(|| before.name.clone());
    let species = identity.species.or_else(|| before.species.clone());
    let cultivar = identity.cultivar.or_else(|| before.cultivar.clone());

    let (after, updated) = store
        .transaction(|store| {
            store.update_profile(id, &name, species.as_deref(), cultivar.as_deref())?;
            let after = store
                .profile(id)?
                .ok_or_else(|| format!("there is no profile with id {id}"))?;

            let mut updated = Vec::new();
            for (field, old, new) in [
                ("name", json!(before.name.clone()), json!(after.name)),
                (
                    "species",
                    json!(before.species.clone()),
                    json!(after.species),
                ),
                (
                    "cultivar",
                    json!(before.cultivar.clone()),
                    json!(after.cultivar),
                ),
            ] {
                if old == new {
                    continue;
                }
                changes::record(
                    store,
                    &changes::NewChange {
                        profile_id: id,
                        entity_type: "profile",
                        entity_id: id,
                        field,
                        old: Some(old),
                        new: Some(new),
                        actor: changes::ACTOR_AGENT,
                        source_type: Some("agent"),
                        source_ref: None,
                    },
                )?;
                updated.push(field);
            }

            Ok((after, updated))
        })
        .map_err(|error| error.to_string())?;

    envelope([
        ("profile".to_string(), json!(id)),
        ("profile_name".to_string(), json!(after.name)),
        ("updated".to_string(), json!(updated)),
    ])
}

fn copy_profile(id: &str, args: &[String]) -> CliResult<String> {
    let id: i64 = id
        .parse()
        .map_err(|_| usage("The profile id must be a number."))?;
    let identity = parse_identity_flags(args)?;
    let store = open_store_for_writing()?;
    let new_id = store.copy_profile_named(id, identity.name.as_deref())?;

    if identity.species.is_some() || identity.cultivar.is_some() {
        let copied = store
            .profile(new_id)?
            .ok_or_else(|| "the copied profile disappeared".to_string())?;
        let species = identity.species.or_else(|| copied.species.clone());
        let cultivar = identity.cultivar.or_else(|| copied.cultivar.clone());
        store.update_profile(
            new_id,
            &copied.name,
            species.as_deref(),
            cultivar.as_deref(),
        )?;
    }

    let copied = store
        .profile(new_id)?
        .ok_or_else(|| "the copied profile disappeared".to_string())?;
    envelope([
        ("profile".to_string(), json!(copied.id)),
        ("profile_name".to_string(), json!(copied.name)),
        ("copied_from".to_string(), json!(id)),
    ])
}

fn list_attributes() -> CliResult<String> {
    let store = open_store()?;
    definitions_json(&store)
}

fn set(id: &str, check_only: bool) -> CliResult<String> {
    let profile_id: i64 = id
        .parse()
        .map_err(|_| usage("The profile id must be a number."))?;

    let mut text = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut text).map_err(|error| CliError {
        message: format!("could not read stdin: {error}"),
        code: 1,
    })?;

    if text.trim().is_empty() {
        return Err(usage("Expected a JSON document on stdin."));
    }

    let store = open_store_for_writing()?;
    let proposal = proposal::parse_for_profile(&store, profile_id, &text)?;
    let report = if check_only {
        proposal::check(&store, &proposal)?
    } else {
        proposal::apply(&store, &proposal)?
    };

    report_json(&proposal, check_only, report)
}

fn import(path: &str, check_only: bool) -> CliResult<String> {
    let store = open_store_for_writing()?;
    let proposal = proposal::load_file(&store, std::path::Path::new(path))?;

    let report = if check_only {
        proposal::check(&store, &proposal)?
    } else {
        let report = proposal::apply(&store, &proposal)?;

        // A proposal from the inbox should not be offered again after it has been applied.
        let path = std::path::Path::new(path);
        if proposal::inbox().is_some_and(|inbox| path.parent() == Some(inbox.as_path()))
            && let Err(error) = proposal::archive(path)
        {
            eprintln!("could not archive {}: {error}", path.display());
        }

        report
    };

    report_json(&proposal, check_only, report)
}

fn report_json(
    proposal: &proposal::Proposal,
    check_only: bool,
    report: Vec<String>,
) -> CliResult<String> {
    envelope([
        ("profile".to_string(), json!(proposal.profile_id)),
        ("profile_name".to_string(), json!(proposal.profile_name)),
        ("checked".to_string(), json!(check_only)),
        ("applied".to_string(), json!(!check_only)),
        ("changes".to_string(), json!(report)),
    ])
}

/// Opens the database for reading only: no migration, no seeding, and a clear error when the
/// file does not exist yet or was written by another schema.
fn open_store() -> CliResult<Store> {
    require_database()?;
    let store = Store::open_read_only()?;
    check_schema(&store)?;
    Ok(store)
}

/// Import writes, so it needs a read-write handle. Everything else stays read-only.
fn open_store_for_writing() -> CliResult<Store> {
    require_database()?;
    let store = Store::open_read_write()?;
    check_schema(&store)?;
    Ok(store)
}

fn database_path() -> CliResult<std::path::PathBuf> {
    Store::default_path().ok_or_else(|| CliError {
        message: "No data directory: set XDG_DATA_HOME or HOME.".to_string(),
        code: 1,
    })
}

/// Checks for the file before opening it: SQLite's own error is less useful than this one.
fn require_database() -> CliResult<()> {
    let path = database_path()?;
    if !path.exists() {
        return Err(CliError {
            message: format!(
                "No database at {}. Open OmaGrow once to create it.",
                path.display()
            ),
            code: 1,
        });
    }
    Ok(())
}

fn check_schema(store: &Store) -> CliResult<()> {
    let version = store.schema_version()?;

    if version != Store::SCHEMA_VERSION {
        return Err(CliError {
            message: format!(
                "The database has schema {version}, but this build expects {}. Open OmaGrow once \
                 with the same version to update it.",
                Store::SCHEMA_VERSION
            ),
            code: 1,
        });
    }

    Ok(())
}

fn profiles_json(store: &Store) -> CliResult<String> {
    let profiles: Vec<Value> = store
        .profiles()?
        .iter()
        .map(|profile| {
            json!({
                "id": profile.id,
                "name": profile.name,
                "species": profile.species,
                "cultivar": profile.cultivar,
            })
        })
        .collect();

    envelope([("profiles".to_string(), Value::Array(profiles))])
}

fn profile_json(store: &Store, id: i64) -> CliResult<String> {
    let Some(details) = store.profile_details(id)? else {
        return Err(CliError {
            message: format!("No profile with id {id}."),
            code: 1,
        });
    };

    let definitions: HashMap<String, AttributeDefinition> = store
        .attribute_definitions()?
        .into_iter()
        .map(|definition| (definition.key.clone(), definition))
        .collect();

    let stages = store
        .stages(id)?
        .iter()
        .map(|stage| stage_json(store, stage))
        .collect::<CliResult<Vec<Value>>>()?;
    let requirements = store
        .requirements(id)?
        .iter()
        .map(|requirement| requirement_json(store, requirement))
        .collect::<CliResult<Vec<Value>>>()?;
    let timing = store
        .timing_rules(id)?
        .iter()
        .map(|rule| timing_json(store, rule))
        .collect::<CliResult<Vec<Value>>>()?;
    let attributes = store
        .attributes(id)?
        .iter()
        .map(|attribute| attribute_json(store, attribute, &definitions))
        .collect::<CliResult<Vec<Value>>>()?;

    let mut provenance = Map::new();
    for item in store.provenance_for("profile", id)? {
        provenance.insert(item.field.clone(), provenance_json(&item));
    }

    envelope([
        ("vocabulary".to_string(), vocabulary_json()),
        (
            "profile".to_string(),
            json!({
                "id": details.id,
                "name": details.name,
                "species": details.species,
                "cultivar": details.cultivar,
                "details": {
                    "family": details.family,
                    "genus": details.genus,
                    "lifecycle": details.lifecycle,
                    "life_form": details.life_form,
                    "foliage": details.foliage,
                    "determinacy": details.determinacy,
                    "growth_habit": details.growth_habit,
                    "sun_exposure": details.sun_exposure,
                    "photoperiod_type": details.photoperiod_type,
                    "photoperiod_obligate": details.photoperiod_obligate,
                    "critical_day_length_hours": details.critical_day_length_hours,
                    "water_need": details.water_need,
                    "container_suitable": details.container_suitable,
                    "min_container_volume_l_min": details.min_container_volume_l_min,
                    "min_container_volume_l_max": details.min_container_volume_l_max,
                    "days_to_maturity_min": details.days_to_maturity_min,
                    "days_to_maturity_max": details.days_to_maturity_max,
                    "notes": details.notes,
                },
                "provenance": Value::Object(provenance),
                "stages": stages,
                "requirements": requirements,
                "timing_rules": timing,
                "attributes": attributes,
            }),
        ),
    ])
}

fn definitions_json(store: &Store) -> CliResult<String> {
    let definitions: Vec<Value> = store
        .attribute_definitions()?
        .into_iter()
        .map(|definition| {
            json!({
                "key": definition.key,
                "label": definition.label,
                "description": definition.description,
                "value_type": definition.value_type,
                "unit": definition.unit,
                "category": definition.category,
                "applies_to": definition.applies_to,
                "allowed_stages": split_list(definition.allowed_stages.as_deref()),
                "allowed_lifecycles": split_list(definition.allowed_lifecycles.as_deref()),
                "enum_values": split_list(definition.enum_values.as_deref()),
                "is_custom": definition.is_custom,
            })
        })
        .collect();

    envelope([("definitions".to_string(), Value::Array(definitions))])
}

fn split_list(value: Option<&str>) -> Option<Vec<String>> {
    value.map(|value| {
        value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect()
    })
}

fn stage_json(store: &Store, stage: &Stage) -> CliResult<Value> {
    Ok(json!({
        "id": stage.id,
        "position": stage.position,
        "kind": stage.kind,
        "label": stage.label,
        "duration_days_min": stage.duration_days_min,
        "duration_days_max": stage.duration_days_max,
        "notes": stage.notes,
        "repeats": stage.repeats,
        "provenance": row_provenance(store, "stage", stage.id)?,
    }))
}

fn requirement_json(store: &Store, requirement: &Requirement) -> CliResult<Value> {
    Ok(json!({
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
        "provenance": row_provenance(store, "requirement", requirement.id)?,
    }))
}

fn timing_json(store: &Store, rule: &TimingRule) -> CliResult<Value> {
    Ok(json!({
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
        "provenance": row_provenance(store, "timing_rule", rule.id)?,
    }))
}

fn attribute_json(
    store: &Store,
    attribute: &Attribute,
    definitions: &HashMap<String, AttributeDefinition>,
) -> CliResult<Value> {
    let definition = definitions.get(&attribute.key);

    Ok(json!({
        "id": attribute.id,
        "stage_id": attribute.stage_id,
        "key": attribute.key,
        "label": definition.map(|definition| definition.label.clone()),
        "unit": definition.and_then(|definition| definition.unit.clone()),
        "value_type": definition.map(|definition| definition.value_type.clone()),
        "is_custom": definition.map(|definition| definition.is_custom),
        "value_num": attribute.value_num,
        "value_text": attribute.value_text,
        "value_bool": attribute.value_bool,
        "provenance": row_provenance(store, "attribute", attribute.id)?,
    }))
}

fn row_provenance(store: &Store, entity_type: &str, entity_id: i64) -> CliResult<Option<Value>> {
    Ok(store
        .provenance(entity_type, entity_id, "*")?
        .as_ref()
        .map(provenance_json))
}

fn provenance_json(provenance: &Provenance) -> Value {
    json!({
        "status": provenance.status,
        "source_type": provenance.source_type,
        "source_ref": provenance.source_ref,
        "retrieved_at": provenance.retrieved_at,
        "confidence": provenance.confidence,
        "notes": provenance.notes,
    })
}

fn vocabulary_json() -> Value {
    let units: Map<String, Value> = METRICS
        .iter()
        .map(|metric| (metric.to_string(), json!(metric_unit(metric))))
        .collect();

    json!({
        "lifecycles": LIFECYCLES,
        "life_forms": LIFE_FORMS,
        "foliage": FOLIAGE,
        "determinacy": DETERMINACY,
        "growth_habits": GROWTH_HABITS,
        "sun_exposures": SUN_EXPOSURES,
        "photoperiod_types": PHOTOPERIOD_TYPES,
        "water_needs": WATER_NEEDS,
        "stage_kinds": STAGE_KINDS,
        "metrics": METRICS,
        "units": units,
        "day_parts": DAY_PARTS,
        "timing_kinds": TIMING_KINDS,
        "timing_anchors": TIMING_ANCHORS,
        "value_types": VALUE_TYPES,
        "applies_to": APPLIES_TO,
        "provenance_statuses": PROVENANCE_STATUSES,
        "source_types": SOURCE_TYPES,
        "confidence_levels": CONFIDENCE_LEVELS,
    })
}

fn envelope(body: impl IntoIterator<Item = (String, Value)>) -> CliResult<String> {
    let mut root = Map::new();
    root.insert("api".to_string(), json!(proposal::API_VERSION));
    root.insert("app".to_string(), json!("omagrow"));
    root.insert("app_version".to_string(), json!(env!("CARGO_PKG_VERSION")));
    root.insert("guide".to_string(), json!("omagrow agent guide"));

    for (key, value) in body {
        root.insert(key, value);
    }

    serde_json::to_string(&Value::Object(root)).map_err(|error| CliError {
        message: format!("could not build JSON: {error}"),
        code: 1,
    })
}

/// The guide without its YAML frontmatter, which only a skill loader needs.
fn guide_text() -> &'static str {
    let Some(rest) = GUIDE.strip_prefix("---\n") else {
        return GUIDE;
    };

    match rest.find("\n---\n") {
        Some(end) => rest[end + 5..].trim_start(),
        None => GUIDE,
    }
}

fn usage_text() -> String {
    format!(
        "OmaGrow — plant, garden and grow journal\n\
         \n\
         Usage:\n\
         \x20 omagrow                     Open the app\n\
         \x20 omagrow agent guide         Print the agent contract\n\
         \x20 omagrow profile list        List profiles as JSON\n\
         \x20 omagrow profile show <id>   Show one profile as JSON\n\
         \x20 omagrow profile create --name NAME  Create a profile\n\
         \x20 omagrow profile update <id> [--name|--species|--cultivar]\n\
         \x20                             Change the identity of a profile\n\
         \x20 omagrow profile copy <id> [--name|--species|--cultivar]\n\
         \x20                             Copy a profile with its plant data\n\
         \x20 omagrow attributes list     List the attribute registry as JSON\n\
         \x20 omagrow import <file>       Apply a proposal, or check it with --check\n\
         \x20 omagrow set <id>            Read that JSON from stdin and write it directly\n\
         \x20 omagrow --help              Show this help\n\
         \x20 omagrow --version           Show the version\n\
         \n\
         Agent commands read the same database as the app and print JSON on stdout.\n\
         Exit codes: 0 success, 1 database or lookup error, 2 usage error.\n\
         \n\
         Version {}.",
        env!("CARGO_PKG_VERSION")
    )
}

#[cfg(test)]
mod tests {
    use super::{
        GUIDE, definitions_json, dispatch, guide_text, parse_identity_flags, profile_json,
        profiles_json,
    };
    use crate::plant_data::{CONFIDENCE_LEVELS, Provenance, Requirement, Stage, TimingRule};
    use crate::store::Store;
    use serde_json::{Value, json};

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn the_bare_command_starts_the_app() {
        assert!(dispatch(&[]).is_none());
    }

    #[test]
    fn parses_profile_identity_flags() {
        let identity = parse_identity_flags(&args(&[
            "--name",
            "Ghost Pepper",
            "--species",
            "Capsicum chinense",
            "--cultivar",
            "Bhut Jolokia",
        ]))
        .unwrap();

        assert_eq!(identity.name.as_deref(), Some("Ghost Pepper"));
        assert_eq!(identity.species.as_deref(), Some("Capsicum chinense"));
        assert_eq!(identity.cultivar.as_deref(), Some("Bhut Jolokia"));

        assert!(parse_identity_flags(&args(&["--unknown", "x"])).is_err());
        assert!(parse_identity_flags(&args(&["--name"])).is_err());
        assert!(parse_identity_flags(&args(&["--name", "--species"])).is_err());
        assert!(parse_identity_flags(&args(&["--name", "A", "--name", "B"])).is_err());
    }

    #[test]
    fn help_and_version_are_plain_text() {
        let help = dispatch(&args(&["--help"])).unwrap().unwrap();
        assert!(help.contains("omagrow profile show"));
        assert!(help.contains("omagrow attributes list"));

        let version = dispatch(&args(&["version"])).unwrap().unwrap();
        assert_eq!(version, format!("omagrow {}", env!("CARGO_PKG_VERSION")));

        let show_usage = dispatch(&args(&["profile", "show", "--help"]))
            .unwrap()
            .unwrap();
        assert!(show_usage.contains("Usage: omagrow profile show <id>"));
    }

    #[test]
    fn rejects_unknown_commands_and_missing_ids() {
        for items in [
            vec!["nope"],
            vec!["agent"],
            vec!["agent", "other"],
            vec!["profile"],
            vec!["profile", "show"],
            vec!["profile", "show", "abc"],
            vec!["profile", "show", "1", "extra"],
            vec!["profile", "list", "extra"],
            vec!["attributes"],
            vec!["attributes", "other"],
            vec!["--help", "extra"],
            vec!["version", "extra"],
        ] {
            let error = dispatch(&args(&items)).unwrap().unwrap_err();
            assert_eq!(error.code, 2, "{items:?}");
        }
    }

    #[test]
    fn prints_the_guide_without_frontmatter() {
        let body = GUIDE.split_once("\n---\n").unwrap().1.trim_start();
        assert_eq!(guide_text(), body);
        assert!(guide_text().starts_with("# OmaGrow"));
        assert!(!guide_text().contains("name: omagrow"));
    }

    #[test]
    fn lists_profiles_with_ids() {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .create_profile("Ghost Pepper", Some("Capsicum chinense"), None)
            .unwrap();

        let json: Value = serde_json::from_str(&profiles_json(&store).unwrap()).unwrap();
        assert_eq!(json["api"], 1);
        assert_eq!(json["profiles"][0]["id"], id);
        assert_eq!(json["profiles"][0]["species"], "Capsicum chinense");
    }

    #[test]
    fn shows_a_profile_with_its_rows_and_provenance() {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .create_profile("Ghost Pepper", Some("Capsicum chinense"), None)
            .unwrap();
        store
            .update_profile_details(&crate::plant_data::ProfileDetails {
                id,
                name: "Ghost Pepper".to_string(),
                species: Some("Capsicum chinense".to_string()),
                cultivar: None,
                family: Some("Solanaceae".to_string()),
                genus: None,
                lifecycle: Some("perennial".to_string()),
                life_form: None,
                foliage: None,
                determinacy: None,
                growth_habit: None,
                sun_exposure: Some("full_sun".to_string()),
                photoperiod_type: None,
                photoperiod_obligate: None,
                critical_day_length_hours: None,
                water_need: Some("medium".to_string()),
                container_suitable: Some(true),
                min_container_volume_l_min: None,
                min_container_volume_l_max: None,
                days_to_maturity_min: None,
                days_to_maturity_max: None,
                notes: None,
            })
            .unwrap();

        let stage = store
            .create_stage(&Stage {
                id: 0,
                profile_id: id,
                position: 1,
                kind: "flowering".to_string(),
                label: "Flowering".to_string(),
                duration_days_min: Some(30.0),
                duration_days_max: Some(45.0),
                notes: None,
                repeats: None,
            })
            .unwrap();
        store
            .set_provenance(&Provenance {
                entity_type: "stage".to_string(),
                entity_id: stage,
                field: "*".to_string(),
                status: "confirmed".to_string(),
                source_type: Some("user".to_string()),
                source_ref: None,
                retrieved_at: None,
                confidence: None,
                notes: None,
            })
            .unwrap();
        store
            .set_requirement(&Requirement {
                id: 0,
                profile_id: id,
                stage_id: Some(stage),
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
        store
            .create_timing_rule(&TimingRule {
                id: 0,
                profile_id: id,
                stage_id: None,
                kind: "sow_indoor".to_string(),
                anchor: "last_frost".to_string(),
                offset_min_days: Some(-56),
                offset_max_days: Some(-42),
                month_start: None,
                month_end: None,
                day_length_hours: None,
                gdd_base_c: None,
                gdd_target: None,
                repeat_days: None,
                notes: None,
            })
            .unwrap();
        store
            .set_provenance(&Provenance {
                entity_type: "profile".to_string(),
                entity_id: id,
                field: "family".to_string(),
                status: "estimated".to_string(),
                source_type: Some("website".to_string()),
                source_ref: Some("https://example.org".to_string()),
                retrieved_at: None,
                confidence: Some("high".to_string()),
                notes: None,
            })
            .unwrap();

        let json: Value = serde_json::from_str(&profile_json(&store, id).unwrap()).unwrap();
        let profile = &json["profile"];
        assert_eq!(profile["name"], "Ghost Pepper");
        assert_eq!(profile["details"]["family"], "Solanaceae");
        assert_eq!(profile["stages"][0]["label"], "Flowering");
        assert_eq!(profile["requirements"][0]["metric"], "air_temp");
        assert_eq!(profile["requirements"][0]["stage_id"], stage);
        assert_eq!(profile["timing_rules"][0]["kind"], "sow_indoor");
        assert_eq!(profile["provenance"]["family"]["status"], "estimated");
        assert_eq!(
            profile["provenance"]["family"]["source_ref"],
            "https://example.org"
        );
        assert_eq!(profile["stages"][0]["provenance"]["status"], "confirmed");
        assert_eq!(profile["stages"][0]["provenance"]["source_type"], "user");
        assert_eq!(
            json["vocabulary"]["confidence_levels"],
            json!(CONFIDENCE_LEVELS)
        );
        assert_eq!(json["vocabulary"]["units"]["air_temp"], "°C");
        assert_eq!(json["vocabulary"]["units"]["water_l_week"], "L/plant/week");
    }

    #[test]
    fn lists_the_attribute_registry_with_enum_values() {
        let store = Store::open_in_memory().unwrap();

        let json: Value = serde_json::from_str(&definitions_json(&store).unwrap()).unwrap();
        let definitions = json["definitions"].as_array().unwrap();
        let climate = definitions
            .iter()
            .find(|definition| definition["key"] == "climate_type")
            .expect("climate_type is seeded");

        assert_eq!(climate["applies_to"], "profile");
        assert_eq!(climate["category"], "Climate");
        assert_eq!(climate["enum_values"][0], "tropical");
        assert!(climate["description"].is_string());
        assert_eq!(climate["is_custom"], false);
    }

    #[test]
    fn reports_a_missing_profile() {
        let store = Store::open_in_memory().unwrap();
        let error = profile_json(&store, 404).unwrap_err();
        assert_eq!(error.code, 1);
    }
}
