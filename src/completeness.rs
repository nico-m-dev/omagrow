use std::collections::HashMap;

use crate::plant_data::{Attribute, ProfileDetails, Requirement, Stage, TimingRule};
use crate::store::Store;

/// The answer for one item: known, deliberately not applicable, or still missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckState {
    Done,
    NotApplicable,
    Missing,
}

impl CheckState {
    /// A deliberate "not applicable" is an answer; it must pass the advice gate.
    pub fn answered(self) -> bool {
        self != Self::Missing
    }
}

/// One item in a tier.
#[derive(Clone, Debug, PartialEq)]
pub struct Check {
    pub label: &'static str,
    pub state: CheckState,
}

impl Check {
    pub fn done(&self) -> bool {
        self.state.answered()
    }

    pub fn not_applicable(&self) -> bool {
        self.state == CheckState::NotApplicable
    }
}

/// The provenance statuses completeness needs, keyed by entity and field.
#[derive(Clone, Debug, Default)]
pub struct ProvenanceView {
    statuses: HashMap<(String, i64, String), String>,
}

impl ProvenanceView {
    pub fn insert(&mut self, entity_type: &str, entity_id: i64, field: &str, status: &str) {
        self.statuses.insert(
            (entity_type.to_string(), entity_id, field.to_string()),
            status.to_string(),
        );
    }

    fn not_applicable(&self, entity_type: &str, entity_id: i64, field: &str) -> bool {
        self.statuses
            .get(&(entity_type.to_string(), entity_id, field.to_string()))
            .is_some_and(|status| status == "not_applicable")
    }
}

/// The completeness of one profile.
#[derive(Clone, Debug, PartialEq)]
pub struct Completeness {
    /// What the app needs before it can give advice.
    pub ready: Vec<Check>,
    /// What makes the advice sharper.
    pub useful: Vec<Check>,
    /// How many long-tail values are filled in.
    pub attributes: usize,
    /// Long-tail values the user declared not applicable.
    pub attributes_not_applicable: usize,
}

impl Completeness {
    pub fn ready_done(&self) -> usize {
        self.ready.iter().filter(|check| check.done()).count()
    }

    pub fn ready_total(&self) -> usize {
        self.ready.len()
    }

    pub fn useful_done(&self) -> usize {
        self.useful.iter().filter(|check| check.done()).count()
    }

    pub fn useful_total(&self) -> usize {
        self.useful.len()
    }

    pub fn is_ready(&self) -> bool {
        self.ready.iter().all(Check::done)
    }

    /// The essential items still missing, in the order they are checked.
    pub fn missing_ready(&self) -> Vec<&'static str> {
        self.ready
            .iter()
            .filter(|check| check.state == CheckState::Missing)
            .map(|check| check.label)
            .collect()
    }

    /// The useful details still missing.
    pub fn missing_useful(&self) -> Vec<&'static str> {
        self.useful
            .iter()
            .filter(|check| check.state == CheckState::Missing)
            .map(|check| check.label)
            .collect()
    }

    /// The essentials the user declared not applicable.
    pub fn not_applicable_ready(&self) -> Vec<&'static str> {
        self.ready
            .iter()
            .filter(|check| check.not_applicable())
            .map(|check| check.label)
            .collect()
    }

    /// Everything is filled in, not only the essentials.
    pub fn is_complete(&self) -> bool {
        self.is_ready() && self.useful.iter().all(Check::done)
    }

    /// Progress through the essentials, for a progress bar.
    pub fn fraction(&self) -> f64 {
        if self.ready.is_empty() {
            return 1.0;
        }

        self.ready_done() as f64 / self.ready_total() as f64
    }

    /// One line for a card: "All 7 essentials (1 not applicable) · 3 of 6 useful details · 4 extra values".
    pub fn summary(&self) -> String {
        let ready_not_applicable = self.not_applicable_ready().len();
        let essentials = if self.is_ready() {
            if ready_not_applicable == 0 {
                format!("All {} essentials", self.ready_total())
            } else {
                format!(
                    "All {} essentials ({ready_not_applicable} not applicable)",
                    self.ready_total()
                )
            }
        } else if ready_not_applicable == 0 {
            format!("{} of {} essentials", self.ready_done(), self.ready_total())
        } else {
            format!(
                "{} of {} essentials ({ready_not_applicable} not applicable)",
                self.ready_done(),
                self.ready_total()
            )
        };

        let useful_not_applicable = self
            .useful
            .iter()
            .filter(|check| check.not_applicable())
            .count();
        let useful = if useful_not_applicable == 0 {
            format!(
                "{} of {} useful details",
                self.useful_done(),
                self.useful_total()
            )
        } else {
            format!(
                "{} of {} useful details ({useful_not_applicable} not applicable)",
                self.useful_done(),
                self.useful_total()
            )
        };

        let attributes = if self.attributes_not_applicable == 0 {
            format!("{} extra values", self.attributes)
        } else {
            format!(
                "{} extra values ({} not applicable)",
                self.attributes, self.attributes_not_applicable
            )
        };

        format!("{essentials} · {useful} · {attributes}")
    }
}

/// Reads the data for one profile and scores it.
pub fn load(store: &Store, profile_id: i64) -> crate::store::Result<Completeness> {
    let Some(details) = store.profile_details(profile_id)? else {
        return Ok(Completeness {
            ready: Vec::new(),
            useful: Vec::new(),
            attributes: 0,
            attributes_not_applicable: 0,
        });
    };

    let stages = store.stages(profile_id)?;
    let requirements = store.requirements(profile_id)?;
    let timing = store.timing_rules(profile_id)?;
    let attributes = store.attributes(profile_id)?;

    let mut provenance = ProvenanceView::default();
    for item in store.provenance_for("profile", profile_id)? {
        provenance.insert(&item.entity_type, item.entity_id, &item.field, &item.status);
    }
    for requirement in &requirements {
        for item in store.provenance_for("requirement", requirement.id)? {
            provenance.insert(&item.entity_type, item.entity_id, &item.field, &item.status);
        }
    }
    for rule in &timing {
        for item in store.provenance_for("timing_rule", rule.id)? {
            provenance.insert(&item.entity_type, item.entity_id, &item.field, &item.status);
        }
    }
    for attribute in &attributes {
        for item in store.provenance_for("attribute", attribute.id)? {
            provenance.insert(&item.entity_type, item.entity_id, &item.field, &item.status);
        }
    }

    Ok(profile_completeness_with(
        &details,
        &stages,
        &requirements,
        &timing,
        &attributes,
        &provenance,
    ))
}

/// Scores a profile from its rows and the provenance that says which values are deliberate.
pub fn profile_completeness_with(
    details: &ProfileDetails,
    stages: &[Stage],
    requirements: &[Requirement],
    timing: &[TimingRule],
    attributes: &[Attribute],
    provenance: &ProvenanceView,
) -> Completeness {
    let metric_state = |metric: &str| -> CheckState {
        let rows: Vec<&Requirement> = requirements
            .iter()
            .filter(|requirement| requirement.metric == metric)
            .collect();
        if rows.is_empty() {
            return CheckState::Missing;
        }
        if rows
            .iter()
            .any(|requirement| !provenance.not_applicable("requirement", requirement.id, "*"))
        {
            CheckState::Done
        } else {
            CheckState::NotApplicable
        }
    };
    let detail_state = |field: &str, present: bool| -> CheckState {
        if provenance.not_applicable("profile", details.id, field) {
            CheckState::NotApplicable
        } else if present {
            CheckState::Done
        } else {
            CheckState::Missing
        }
    };
    let combined = |states: &[CheckState]| -> CheckState {
        if states.contains(&CheckState::Done) {
            CheckState::Done
        } else if states.contains(&CheckState::NotApplicable) {
            CheckState::NotApplicable
        } else {
            CheckState::Missing
        }
    };
    let timing_state = |rules: Vec<&TimingRule>| -> CheckState {
        if rules.is_empty() {
            return CheckState::Missing;
        }
        if rules
            .iter()
            .any(|rule| !provenance.not_applicable("timing_rule", rule.id, "*"))
        {
            CheckState::Done
        } else {
            CheckState::NotApplicable
        }
    };

    let optimal_air: Vec<&Requirement> = requirements
        .iter()
        .filter(|requirement| requirement.metric == "air_temp")
        .collect();
    let optimal_temperature = if !optimal_air.is_empty()
        && optimal_air
            .iter()
            .all(|requirement| provenance.not_applicable("requirement", requirement.id, "*"))
    {
        CheckState::NotApplicable
    } else if optimal_air
        .iter()
        .any(|requirement| requirement.opt_min.is_some() || requirement.opt_max.is_some())
    {
        CheckState::Done
    } else {
        CheckState::Missing
    };

    let water = combined(&[
        detail_state("water_need", details.water_need.is_some()),
        metric_state("water_l_week"),
        metric_state("water_mm_week"),
    ]);
    let light = combined(&[
        detail_state("sun_exposure", details.sun_exposure.is_some()),
        detail_state("photoperiod_type", details.photoperiod_type.is_some()),
        metric_state("photoperiod"),
        metric_state("light_dli"),
        metric_state("direct_sun_hours"),
    ]);
    let container = combined(&[
        detail_state("container_suitable", details.container_suitable.is_some()),
        detail_state(
            "min_container_volume_l_min",
            details.min_container_volume_l_min.is_some(),
        ),
        detail_state(
            "min_container_volume_l_max",
            details.min_container_volume_l_max.is_some(),
        ),
    ]);
    let maturity = combined(&[
        detail_state(
            "days_to_maturity_min",
            details.days_to_maturity_min.is_some(),
        ),
        detail_state(
            "days_to_maturity_max",
            details.days_to_maturity_max.is_some(),
        ),
    ]);

    // A stage with a timing rule has a known start even without a duration.
    let stage_is_dated = |stage: &Stage| {
        stage.duration_days_min.is_some()
            || stage.duration_days_max.is_some()
            || timing.iter().any(|rule| rule.stage_id == Some(stage.id))
    };
    let stages_have_durations = !stages.is_empty() && stages.iter().all(stage_is_dated);

    let sowing_rules: Vec<&TimingRule> = timing
        .iter()
        .filter(|rule| rule.kind.starts_with("sow"))
        .collect();
    let harvest_rules: Vec<&TimingRule> = timing
        .iter()
        .filter(|rule| rule.kind == "harvest")
        .collect();

    let ready = vec![
        Check {
            label: "species or variety",
            state: detail_state(
                "species",
                details
                    .species
                    .as_deref()
                    .is_some_and(|species| !species.trim().is_empty()),
            ),
        },
        Check {
            label: "lifecycle",
            state: detail_state("lifecycle", details.lifecycle.is_some()),
        },
        Check {
            label: "optimal temperature",
            state: optimal_temperature,
        },
        Check {
            label: "water need",
            state: water,
        },
        Check {
            label: "light",
            state: light,
        },
        Check {
            label: "a growing stage",
            state: if stages.is_empty() {
                CheckState::Missing
            } else {
                CheckState::Done
            },
        },
        Check {
            label: "days to maturity",
            state: maturity,
        },
    ];

    let useful = vec![
        Check {
            label: "container suitability",
            state: container,
        },
        Check {
            label: "soil pH",
            state: metric_state("soil_ph"),
        },
        Check {
            label: "humidity",
            state: metric_state("rh"),
        },
        Check {
            label: "stage durations",
            state: if stages_have_durations {
                CheckState::Done
            } else {
                CheckState::Missing
            },
        },
        Check {
            label: "sowing timing",
            state: timing_state(sowing_rules),
        },
        Check {
            label: "harvest timing",
            state: timing_state(harvest_rules),
        },
    ];

    let attributes_not_applicable = attributes
        .iter()
        .filter(|attribute| provenance.not_applicable("attribute", attribute.id, "*"))
        .count();

    Completeness {
        ready,
        useful,
        attributes: attributes.len() - attributes_not_applicable,
        attributes_not_applicable,
    }
}

#[cfg(test)]
mod tests {
    use super::{Completeness, ProvenanceView, profile_completeness_with};
    use crate::plant_data::{Attribute, ProfileDetails, Requirement, Stage, TimingRule};

    fn score(
        details: &ProfileDetails,
        stages: &[Stage],
        requirements: &[Requirement],
        timing: &[TimingRule],
        attributes: &[Attribute],
    ) -> Completeness {
        profile_completeness_with(
            details,
            stages,
            requirements,
            timing,
            attributes,
            &ProvenanceView::default(),
        )
    }

    fn empty_details() -> ProfileDetails {
        ProfileDetails {
            id: 1,
            name: "Ghost Pepper".to_string(),
            species: None,
            cultivar: None,
            family: None,
            genus: None,
            lifecycle: None,
            life_form: None,
            foliage: None,
            determinacy: None,
            growth_habit: None,
            sun_exposure: None,
            photoperiod_type: None,
            photoperiod_obligate: None,
            critical_day_length_hours: None,
            water_need: None,
            container_suitable: None,
            min_container_volume_l_min: None,
            min_container_volume_l_max: None,
            days_to_maturity_min: None,
            days_to_maturity_max: None,
            notes: None,
        }
    }

    fn stage(duration: Option<f64>) -> Stage {
        Stage {
            id: 1,
            profile_id: 1,
            position: 1,
            kind: "flowering".to_string(),
            label: "Flowering".to_string(),
            duration_days_min: duration,
            duration_days_max: None,
            notes: None,
            repeats: None,
        }
    }

    fn requirement(metric: &str) -> Requirement {
        Requirement {
            id: 1,
            profile_id: 1,
            stage_id: None,
            metric: metric.to_string(),
            day_part: "day".to_string(),
            min: None,
            max: None,
            opt_min: Some(20.0),
            opt_max: Some(26.0),
            survival_min: None,
            survival_max: None,
        }
    }

    fn timing(kind: &str) -> TimingRule {
        TimingRule {
            id: 1,
            profile_id: 1,
            stage_id: None,
            kind: kind.to_string(),
            anchor: "last_frost".to_string(),
            offset_min_days: Some(-14),
            offset_max_days: None,
            month_start: None,
            month_end: None,
            day_length_hours: None,
            gdd_base_c: None,
            gdd_target: None,
            repeat_days: None,
            notes: None,
        }
    }

    fn attribute() -> Attribute {
        Attribute {
            id: 1,
            profile_id: 1,
            stage_id: None,
            key: "climate_type".to_string(),
            value_num: None,
            value_text: Some("tropical".to_string()),
            value_bool: None,
        }
    }

    #[test]
    fn an_empty_profile_is_not_ready() {
        let completeness = score(&empty_details(), &[], &[], &[], &[]);

        assert_eq!(completeness.ready_done(), 0);
        assert_eq!(completeness.ready_total(), 7);
        assert_eq!(completeness.useful_done(), 0);
        assert_eq!(completeness.attributes, 0);
        assert!(!completeness.is_ready());
        assert_eq!(
            completeness.missing_ready(),
            vec![
                "species or variety",
                "lifecycle",
                "optimal temperature",
                "water need",
                "light",
                "a growing stage",
                "days to maturity",
            ]
        );
    }

    #[test]
    fn essentials_come_from_details_requirements_and_stages() {
        let mut details = empty_details();
        details.species = Some("Capsicum chinense".to_string());
        details.lifecycle = Some("perennial".to_string());
        details.water_need = Some("medium".to_string());
        details.sun_exposure = Some("full_sun".to_string());
        details.days_to_maturity_min = Some(80.0);
        details.days_to_maturity_max = Some(100.0);

        let completeness = score(
            &details,
            &[stage(Some(30.0))],
            &[requirement("air_temp")],
            &[],
            &[],
        );

        assert!(completeness.is_ready());
        assert_eq!(completeness.useful_done(), 1, "only the stage duration");
        assert_eq!(completeness.fraction(), 1.0);
        assert!(completeness.missing_ready().is_empty());
        assert!(completeness.summary().starts_with("All 7 essentials"));
    }

    #[test]
    fn requirements_can_stand_in_for_details() {
        let completeness = score(
            &empty_details(),
            &[stage(None)],
            &[requirement("air_temp"), requirement("water_l_week")],
            &[],
            &[],
        );

        assert_eq!(completeness.ready_done(), 3, "temperature, water and stage");
        assert_eq!(completeness.missing_ready().len(), 4);
        assert!(!completeness.is_ready());
        assert_eq!(completeness.useful_done(), 0, "a stage without a duration");
    }

    #[test]
    fn essentials_complete_is_not_the_same_as_complete() {
        let mut details = empty_details();
        details.species = Some("Capsicum chinense".to_string());
        details.lifecycle = Some("perennial".to_string());
        details.water_need = Some("medium".to_string());
        details.sun_exposure = Some("full_sun".to_string());
        details.days_to_maturity_min = Some(80.0);

        let completeness = score(
            &details,
            &[stage(None)],
            &[requirement("air_temp")],
            &[],
            &[],
        );

        assert!(completeness.is_ready());
        assert!(!completeness.is_complete());
        assert_eq!(completeness.missing_useful().len(), 6);
    }

    #[test]
    fn the_useful_tier_and_long_tail_count() {
        let mut details = empty_details();
        details.container_suitable = Some(true);

        let completeness = score(
            &details,
            &[stage(Some(30.0))],
            &[requirement("soil_ph"), requirement("rh")],
            &[timing("sow_indoor"), timing("harvest")],
            &[attribute()],
        );

        assert_eq!(completeness.useful_done(), 6);
        assert_eq!(completeness.useful_total(), 6);
        assert_eq!(completeness.attributes, 1);
    }

    #[test]
    fn a_not_applicable_value_is_an_answer() {
        let mut details = empty_details();
        details.water_need = Some("medium".to_string());
        details.sun_exposure = Some("full_sun".to_string());
        details.days_to_maturity_min = Some(80.0);

        let mut provenance = ProvenanceView::default();
        provenance.insert("profile", 1, "species", "not_applicable");
        provenance.insert("profile", 1, "lifecycle", "not_applicable");

        let completeness = profile_completeness_with(
            &details,
            &[stage(Some(30.0))],
            &[requirement("air_temp")],
            &[],
            &[],
            &provenance,
        );

        assert!(completeness.is_ready());
        assert_eq!(completeness.ready_done(), 7);
        assert!(completeness.missing_ready().is_empty());
        assert_eq!(
            completeness.not_applicable_ready(),
            vec!["species or variety", "lifecycle"]
        );
        assert!(completeness.summary().contains("2 not applicable"));
    }

    #[test]
    fn a_not_applicable_attribute_is_counted_separately() {
        let mut provenance = ProvenanceView::default();
        provenance.insert("attribute", 1, "*", "not_applicable");

        let completeness =
            profile_completeness_with(&empty_details(), &[], &[], &[], &[attribute()], &provenance);

        assert_eq!(completeness.attributes, 0);
        assert_eq!(completeness.attributes_not_applicable, 1);
        assert!(
            completeness
                .summary()
                .contains("0 extra values (1 not applicable)")
        );
    }

    #[test]
    fn a_stage_with_a_timing_rule_counts_as_dated() {
        let mut prune = timing("prune");
        prune.stage_id = Some(1);

        let completeness = score(&empty_details(), &[stage(None)], &[], &[prune], &[]);

        assert!(
            completeness
                .useful
                .iter()
                .any(|check| check.label == "stage durations" && check.done())
        );
    }

    #[test]
    fn provenance_wins_over_a_stored_value() {
        let mut details = empty_details();
        details.species = Some("Capsicum chinense".to_string());

        let mut provenance = ProvenanceView::default();
        provenance.insert("profile", 1, "species", "not_applicable");
        provenance.insert("requirement", 1, "*", "not_applicable");

        let completeness = profile_completeness_with(
            &details,
            &[],
            &[requirement("air_temp")],
            &[],
            &[],
            &provenance,
        );

        assert!(
            completeness
                .ready
                .iter()
                .any(|check| check.label == "species or variety" && check.not_applicable())
        );
        assert!(
            completeness
                .ready
                .iter()
                .any(|check| check.label == "optimal temperature" && check.not_applicable())
        );
        assert!(completeness.summary().contains("2 not applicable"));
    }
}
