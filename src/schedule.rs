//! The derived schedule of one planting: when each stage is expected and which dated milestones
//! the profile and the frost dates imply.
//!
//! Everything here is an estimate from the profile, the planting's real start and the frost dates
//! in Settings. Nothing is written to the database. A stage with no duration stops the timeline
//! instead of inventing a date; a rule whose anchor cannot be resolved is reported as unknown.

use std::collections::HashMap;

use crate::calendar::Date;
use crate::frost::{self, FrostDates};
use crate::plant_data::{Stage, TimingRule};
use crate::planting::Planting;

/// One stage placed on the calendar, with the uncertainty around its dates.
#[derive(Clone, Debug, PartialEq)]
pub struct StageWindow {
    pub stage: Stage,
    /// The first day the stage can start.
    pub earliest_start: Date,
    /// The last day the stage can start.
    pub latest_start: Date,
    /// The first day the stage can end.
    pub earliest_end: Date,
    /// The last day the stage can end.
    pub latest_end: Date,
    /// The middle of the estimate: a single likely start.
    pub likely_start: Date,
    /// The middle of the estimate: a single likely end.
    pub likely_end: Date,
    /// True when the start was recorded in the diary instead of estimated.
    pub actual_start: bool,
}

impl StageWindow {
    pub fn contains_likely(&self, date: Date) -> bool {
        self.likely_start <= date && date <= self.likely_end
    }
}

/// A dated event, or a window when the source gives a range.
#[derive(Clone, Debug, PartialEq)]
pub struct Milestone {
    pub date: Date,
    /// `Some` when this is a window such as a harvest period.
    pub end: Option<Date>,
    /// The timing kind (`harvest`, `transplant`, `stage`, ...); the UI maps it to a colour.
    pub kind: String,
    pub label: String,
}

impl Milestone {
    pub fn last_day(&self) -> Date {
        self.end.unwrap_or(self.date)
    }

    pub fn is_window(&self) -> bool {
        self.end.is_some_and(|end| end > self.date)
    }
}

/// Everything the planting page needs to draw a timeline and a calendar.
#[derive(Clone, Debug, PartialEq)]
pub struct Schedule {
    pub stages: Vec<StageWindow>,
    pub milestones: Vec<Milestone>,
    /// The stage whose likely window contains today.
    pub current_stage: Option<usize>,
    /// The next milestone that has not fully passed.
    pub next: Option<Milestone>,
    /// Stage labels the profile gives no duration, so they could not be placed.
    pub undated_stages: Vec<String>,
    /// Rules that could not be dated, in readable form.
    pub unknown: Vec<String>,
    pub span_start: Date,
    pub span_end: Date,
}

impl Schedule {
    /// The likely stage on a date, for the calendar's day detail. The later stage wins on a
    /// boundary day, so the day a new stage starts is credited to the new stage.
    pub fn stage_on(&self, date: Date) -> Option<&StageWindow> {
        self.stages
            .iter()
            .rev()
            .find(|window| window.contains_likely(date))
    }
}

/// How many repeated cycles a perennial timeline may draw. Twenty years is far beyond what the
/// page can show legibly and keeps a mistyped date from exploding the schedule.
const MAX_CYCLES: usize = 20;

/// One appearance of a stage in the plan: the cycle it belongs to and its likely start.
///
/// A perennial shows the same stage in every cycle, so a logged start has to be matched to the
/// occurrence the user meant. The plan without corrections gives each occurrence its place; the
/// nearest one to the logged date takes it.
struct Occurrence {
    cycle: usize,
    stage_id: i64,
    start: Date,
}

pub fn build(
    planting: &Planting,
    stages: &[Stage],
    rules: &[TimingRule],
    stage_starts: &[(i64, Date)],
    frost: &FrostDates,
    today: Date,
    lifecycle: Option<&str>,
) -> Schedule {
    // The plan without logged corrections places every stage occurrence in time; that is what a
    // logged date is matched against. Without a correction the plan is the answer.
    let (plan, occurrences) = build_inner(
        planting,
        stages,
        rules,
        &HashMap::new(),
        frost,
        today,
        lifecycle,
    );
    let claims = claim_starts(stage_starts, &occurrences);
    if claims.is_empty() {
        return plan;
    }

    build_inner(planting, stages, rules, &claims, frost, today, lifecycle).0
}

/// Match every logged stage start to the nearest occurrence of that stage, one start per
/// occurrence, oldest first. A date logged for this year's dormancy belongs to this year's block,
/// so an earlier cycle keeps its own dates.
fn claim_starts(logged: &[(i64, Date)], occurrences: &[Occurrence]) -> HashMap<(usize, i64), Date> {
    let mut claims = HashMap::new();

    for (stage_id, date) in logged {
        let matches: Vec<&Occurrence> = occurrences
            .iter()
            .filter(|occurrence| occurrence.stage_id == *stage_id)
            .collect();

        // A stage the plan cannot place (no duration) still becomes a point on the first cycle.
        if matches.is_empty() {
            claims.entry((0, *stage_id)).or_insert(*date);
            continue;
        }

        // The nearest occurrence takes the date. A second date for an occurrence that is already
        // claimed is ignored, so a duplicate log never moves the next cycle.
        let nearest = matches
            .into_iter()
            .min_by_key(|occurrence| (occurrence.start.days_until(*date).abs(), occurrence.start));

        if let Some(occurrence) = nearest
            && !claims.contains_key(&(occurrence.cycle, occurrence.stage_id))
        {
            claims.insert((occurrence.cycle, occurrence.stage_id), *date);
        }
    }

    claims
}

fn build_inner(
    planting: &Planting,
    stages: &[Stage],
    rules: &[TimingRule],
    claims: &HashMap<(usize, i64), Date>,
    frost: &FrostDates,
    today: Date,
    lifecycle: Option<&str>,
) -> (Schedule, Vec<Occurrence>) {
    let mut ordered = stages.to_vec();
    ordered.sort_by_key(|stage| stage.position);

    let mut schedule = Schedule {
        stages: Vec::new(),
        milestones: Vec::new(),
        current_stage: None,
        next: None,
        undated_stages: Vec::new(),
        unknown: Vec::new(),
        span_start: planting.started_on,
        span_end: planting.ended_on.unwrap_or(planting.started_on),
    };

    let mut occurrences = Vec::new();
    let mut earliest = planting.started_on;
    let mut latest = planting.started_on;
    let mut likely = planting.started_on;

    if let Some(skip) = start_index(&planting.start_kind, &ordered) {
        for stage in ordered.iter().skip(skip) {
            let recorded = claims.get(&(0, stage.id)).copied();
            let out_of_order = recorded.is_some_and(|date| {
                schedule
                    .stages
                    .last()
                    .is_some_and(|previous| date < previous.earliest_start)
            });
            let actual = if out_of_order {
                schedule.unknown.push(format!(
                    "{} started before the previous stage; the timeline keeps the profile order",
                    stage.label
                ));
                None
            } else {
                recorded
            };

            if let Some(date) = actual {
                if let Some(previous) = schedule.stages.last_mut() {
                    previous.latest_start = previous.latest_start.min(date);
                    previous.likely_start = previous.likely_start.min(date);
                    previous.earliest_end = date;
                    previous.latest_end = date;
                    previous.likely_end = date;
                }
                earliest = date;
                latest = date;
                likely = date;
            }

            let Some((min, max)) = stage_days(stage) else {
                match actual {
                    Some(date) => {
                        schedule.stages.push(StageWindow {
                            stage: stage.clone(),
                            earliest_start: date,
                            latest_start: date,
                            earliest_end: date,
                            latest_end: date,
                            likely_start: date,
                            likely_end: date,
                            actual_start: true,
                        });
                        schedule.span_start = schedule.span_start.min(date);
                        schedule.span_end = schedule.span_end.max(date);
                        schedule.unknown.push(format!(
                            "{} has no duration; only the started date is known",
                            stage.label
                        ));
                    }
                    None => schedule.undated_stages.push(stage.label.clone()),
                }
                break;
            };

            let average = ((min + max) as f64 / 2.0).round() as i64;
            let window = StageWindow {
                stage: stage.clone(),
                earliest_start: earliest,
                latest_start: latest,
                earliest_end: earliest.add_days(min),
                latest_end: latest.add_days(max),
                likely_start: likely,
                likely_end: likely.add_days(average),
                actual_start: actual.is_some(),
            };
            occurrences.push(Occurrence {
                cycle: 0,
                stage_id: stage.id,
                start: window.likely_start,
            });

            schedule.span_start = schedule.span_start.min(window.earliest_start);
            schedule.span_end = schedule.span_end.max(window.latest_end);
            earliest = window.earliest_end;
            latest = window.latest_end;
            likely = window.likely_end;
            schedule.stages.push(window);
        }
    } else if !ordered.is_empty() {
        schedule.unknown.push(
            "The profile has no stage after germination to start a transplant at".to_string(),
        );
    }

    // A perennial lives through the same cycle again: the stages marked for a yearly repeat return,
    // the establishment stages happen once. A repeated cycle starts on the anniversary of the
    // first cycle, so a cycle that is shorter than a year leaves an honest gap instead of drifting
    // out of season. A single terminal `mature` stage is the exception: it is a continuing adult
    // state, not a yearly event, so it runs until the planting ends (or one year past today). The
    // repeated windows are estimates; a logged stage start belongs to the first cycle (the diary
    // keeps one start per stage).
    let repeating = (lifecycle == Some("perennial"))
        .then(|| repeating_cycle(&ordered, &schedule.stages))
        .flatten();

    if let Some(cycle) = repeating
        && let Some(anchor) = schedule
            .stages
            .iter()
            .find(|window| cycle.iter().any(|stage| stage.id == window.stage.id))
            .map(|window| window.likely_start)
    {
        let continuous_mature = cycle.len() == 1
            && cycle[0].kind == "mature"
            && schedule
                .stages
                .last()
                .is_some_and(|window| window.stage.kind == "mature");

        if continuous_mature {
            let end = planting.ended_on.unwrap_or(today.add_days(365));
            if let Some(window) = schedule.stages.last_mut()
                && end > window.likely_end
            {
                window.earliest_end = end;
                window.latest_end = end;
                window.likely_end = end;
                schedule.span_end = schedule.span_end.max(end);
            }
        } else {
            let target = planting.ended_on.unwrap_or(today);
            let mut headroom_done = planting.ended_on.is_some();
            let mut cycles = 0;
            let mut overlap_reported = false;

            while cycles < MAX_CYCLES {
                if likely >= target {
                    if headroom_done {
                        break;
                    }
                    // One more cycle shows the season that is coming.
                    headroom_done = true;
                }

                cycles += 1;
                let Some(start) =
                    Date::in_year(anchor.year + cycles as i32, anchor.month, anchor.day)
                else {
                    break;
                };

                // A finished planting gets no cycle that only starts after it ended.
                if planting.ended_on.is_some_and(|ended| start > ended) {
                    break;
                }

                if start < earliest && !overlap_reported {
                    schedule.unknown.push(
                        "The yearly cycle is longer than a year; the next cycle starts before the \
                         previous one ends"
                            .to_string(),
                    );
                    overlap_reported = true;
                }

                let mut cycle_earliest = start;
                let mut cycle_latest = start;
                let mut cycle_likely = start;
                let mut first_in_cycle = true;

                for stage in &cycle {
                    let Some((min, max)) = stage_days(stage) else {
                        break;
                    };

                    // A logged start moves this cycle only: the previous stage of the same cycle
                    // ends that day, and the stages after it follow from there.
                    let recorded = claims.get(&(cycles, stage.id)).copied();
                    let recorded = match recorded {
                        Some(date)
                            if !first_in_cycle
                                && schedule
                                    .stages
                                    .last()
                                    .is_some_and(|previous| date < previous.earliest_start) =>
                        {
                            schedule.unknown.push(format!(
                                "{} started before the previous stage; the timeline keeps the \
                                 profile order",
                                stage.label
                            ));
                            None
                        }
                        other => other,
                    };

                    if let Some(date) = recorded {
                        if !first_in_cycle && let Some(previous) = schedule.stages.last_mut() {
                            previous.latest_start = previous.latest_start.min(date);
                            previous.likely_start = previous.likely_start.min(date);
                            previous.earliest_end = date;
                            previous.latest_end = date;
                            previous.likely_end = date;
                        }
                        cycle_earliest = date;
                        cycle_latest = date;
                        cycle_likely = date;
                    }

                    let average = ((min + max) as f64 / 2.0).round() as i64;
                    let window = StageWindow {
                        stage: stage.clone(),
                        earliest_start: cycle_earliest,
                        latest_start: cycle_latest,
                        earliest_end: cycle_earliest.add_days(min),
                        latest_end: cycle_latest.add_days(max),
                        likely_start: cycle_likely,
                        likely_end: cycle_likely.add_days(average),
                        actual_start: recorded.is_some(),
                    };
                    occurrences.push(Occurrence {
                        cycle: cycles,
                        stage_id: stage.id,
                        start: window.likely_start,
                    });

                    cycle_earliest = window.earliest_end;
                    cycle_latest = window.latest_end;
                    cycle_likely = window.likely_end;
                    first_in_cycle = false;
                    schedule.stages.push(window);
                }

                earliest = cycle_earliest;
                latest = cycle_latest;
                likely = cycle_likely;
                schedule.span_end = schedule.span_end.max(latest);
            }
        }
    }

    if let Some(ended_on) = planting.ended_on {
        // A finished planting keeps only the stages that could have started before its end, and a
        // stage that was cut short ends on that day.
        schedule
            .stages
            .retain(|window| window.likely_start <= ended_on);
        for window in &mut schedule.stages {
            window.latest_start = window.latest_start.min(ended_on);
            window.earliest_end = window.earliest_end.min(ended_on);
            window.latest_end = window.latest_end.min(ended_on);
            window.likely_end = window.likely_end.min(ended_on);
        }
        schedule.span_end = schedule
            .stages
            .iter()
            .map(|window| window.latest_end)
            .max()
            .unwrap_or(ended_on)
            .max(ended_on);
    }

    // A cycle longer than a year can overlap the next one. Keep the windows in date order so the
    // day lookup and the timeline agree on which stage comes later; the sort is stable, so equal
    // starts keep profile order.
    schedule
        .stages
        .sort_by_key(|window| (window.likely_start, window.likely_end));

    // A timing rule anchored to a stage uses the first time that stage appears, not a later cycle.
    let stage_dates: HashMap<i64, Date> = {
        let mut dates = HashMap::new();
        for window in &schedule.stages {
            dates.entry(window.stage.id).or_insert(window.likely_start);
        }
        dates
    };

    // A seed planting knows its sowing date. A plant or tree is planted at its start. A
    // transplant knows only the transplant date; a sowing rule can estimate what came before it.
    let mut sowing_date = if matches!(
        planting.start_kind.as_str(),
        "sown_indoor" | "sown_outdoor" | "planted"
    ) {
        Some(planting.started_on)
    } else {
        None
    };
    let mut transplant_date = if matches!(planting.start_kind.as_str(), "transplanted" | "planted")
    {
        Some(planting.started_on)
    } else {
        None
    };

    if transplant_date.is_none() {
        transplant_date = rules
            .iter()
            .find(|rule| rule.kind == "transplant")
            .and_then(|rule| {
                resolve_rule(
                    rule,
                    planting,
                    sowing_date,
                    None,
                    &stage_dates,
                    frost,
                    planting.started_on.year,
                )
                .ok()
                .map(|(start, end)| midpoint(start, end))
            });
    }

    if sowing_date.is_none() {
        sowing_date = rules
            .iter()
            .find(|rule| matches!(rule.kind.as_str(), "sow_indoor" | "sow_outdoor"))
            .and_then(|rule| {
                resolve_rule(
                    rule,
                    planting,
                    None,
                    transplant_date,
                    &stage_dates,
                    frost,
                    planting.started_on.year,
                )
                .ok()
                .map(|(start, end)| midpoint(start, end))
            })
            .filter(|date| *date < planting.started_on);
    }

    schedule.milestones.push(Milestone {
        date: planting.started_on,
        end: None,
        kind: planting.start_kind.clone(),
        label: start_kind_label(&planting.start_kind).to_string(),
    });

    for window in &schedule.stages {
        schedule.milestones.push(Milestone {
            date: window.likely_start,
            end: None,
            kind: "stage".to_string(),
            label: if window.actual_start {
                format!("{} started", window.stage.label)
            } else {
                format!("{} starts", window.stage.label)
            },
        });
    }

    for rule in rules {
        // A perennial repeats its care rules each year, not its sowing or transplant. Stage
        // anchored rules keep their first occurrence, as the stage timeline does.
        let years: Vec<i32> = if lifecycle == Some("perennial")
            && repeats_yearly(&rule.kind)
            && matches!(
                rule.anchor.as_str(),
                "last_frost" | "first_frost" | "calendar"
            ) {
            let last_year = planting.ended_on.unwrap_or(schedule.span_end).year;
            (planting.started_on.year..=last_year).collect()
        } else {
            vec![planting.started_on.year]
        };

        let mut reported = false;
        for year in years {
            match resolve_rule(
                rule,
                planting,
                sowing_date,
                transplant_date,
                &stage_dates,
                frost,
                year,
            ) {
                Ok((start, end)) => {
                    if planting.ended_on.is_some_and(|ended| start > ended) {
                        continue;
                    }

                    let repeats_start = rule_matches_start(rule, planting)
                        && start <= planting.started_on
                        && planting.started_on <= end;
                    if repeats_start {
                        continue;
                    }

                    schedule.milestones.push(Milestone {
                        date: start,
                        end: (end > start).then_some(end),
                        kind: rule.kind.clone(),
                        label: kind_label(&rule.kind),
                    });
                }
                Err(reason) => {
                    if !reported {
                        schedule
                            .unknown
                            .push(format!("{}: {reason}", kind_label(&rule.kind)));
                        reported = true;
                    }
                }
            }
        }
    }

    schedule.milestones.sort_by(|a, b| {
        a.date
            .cmp(&b.date)
            .then_with(|| a.end.cmp(&b.end))
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.label.cmp(&b.label))
    });
    schedule.milestones.dedup_by(|a, b| {
        a.date == b.date && a.end == b.end && a.kind == b.kind && a.label == b.label
    });

    if schedule.stages.is_empty() {
        if let Some(first) = schedule.milestones.first() {
            schedule.span_start = first.date;
        }
        if let Some(last) = schedule.milestones.last() {
            schedule.span_end = last.last_day().max(planting.started_on);
        }
    }

    if planting.ended_on.is_none() {
        schedule.current_stage = schedule
            .stages
            .iter()
            .rposition(|window| window.contains_likely(today));

        schedule.next = schedule
            .milestones
            .iter()
            .find(|milestone| milestone.last_day() >= today)
            .cloned();
    }

    (schedule, occurrences)
}

/// Which stage a planting starts at. A transplant has already germinated, so the germination and
/// seedling stages before it are skipped. A sowing or a planted tuber/tree starts at the first
/// stage the profile describes. `None` when a transplant profile has nothing after germination.
fn start_index(start_kind: &str, stages: &[Stage]) -> Option<usize> {
    if start_kind != "transplanted" {
        return Some(0);
    }

    stages
        .iter()
        .position(|stage| {
            matches!(
                stage.kind.as_str(),
                "establishment" | "vegetative" | "juvenile" | "mature"
            )
        })
        .or_else(|| {
            stages
                .iter()
                .position(|stage| !matches!(stage.kind.as_str(), "germination" | "seedling"))
        })
}

/// The stages a perennial repeats: the ones the profile marks for a yearly repeat, in cycle
/// order. A stage that the first cycle did not place (skipped or after an undated stage) is left
/// out, and a cycle needs a duration for every stage, otherwise it cannot be placed.
fn repeating_cycle(stages: &[Stage], first_cycle: &[StageWindow]) -> Option<Vec<Stage>> {
    let cycle: Vec<Stage> = stages
        .iter()
        .filter(|stage| stage.repeats_each_year())
        .filter(|stage| first_cycle.iter().any(|window| window.stage.id == stage.id))
        .cloned()
        .collect();

    if cycle.is_empty() || cycle.iter().any(|stage| stage_days(stage).is_none()) {
        return None;
    }
    Some(cycle)
}

fn stage_days(stage: &Stage) -> Option<(i64, i64)> {
    let min = stage.duration_days_min.or(stage.duration_days_max)?;
    let max = stage.duration_days_max.unwrap_or(min);

    if !min.is_finite() || !max.is_finite() || min < 0.0 || max < min {
        return None;
    }

    Some((min.round() as i64, max.round() as i64))
}

fn resolve_rule(
    rule: &TimingRule,
    planting: &Planting,
    sowing_date: Option<Date>,
    transplant_date: Option<Date>,
    stage_dates: &HashMap<i64, Date>,
    frost: &FrostDates,
    year: i32,
) -> Result<(Date, Date), String> {
    match rule.anchor.as_str() {
        "last_frost" => {
            let (start, end) = frost::resolve(rule, frost, year)
                .ok_or_else(|| "the last frost date is not set in Settings".to_string())?;
            Ok((start, end))
        }
        "first_frost" => {
            let (start, end) = frost::resolve(rule, frost, year)
                .ok_or_else(|| "the first frost date is not set in Settings".to_string())?;
            if end < planting.started_on {
                frost::resolve(rule, frost, year + 1)
                    .ok_or_else(|| "the first frost date is not set in Settings".to_string())
            } else {
                Ok((start, end))
            }
        }
        "sowing" => Ok(offset_window(
            sowing_date.ok_or_else(|| "the sowing date is unknown".to_string())?,
            rule,
        )),
        "transplant" => transplant_date
            .map(|date| offset_window(date, rule))
            .ok_or_else(|| "the transplant date is unknown".to_string()),
        "stage_start" => {
            let stage_id = rule
                .stage_id
                .ok_or_else(|| "no stage is set for this rule".to_string())?;
            let date = *stage_dates
                .get(&stage_id)
                .ok_or_else(|| "that stage has no estimated date".to_string())?;
            Ok(offset_window(date, rule))
        }
        "calendar" => {
            let (start, end) = calendar_window(rule, year)
                .ok_or_else(|| "no month is set for this rule".to_string())?;
            if end < planting.started_on {
                calendar_window(rule, year + 1)
                    .ok_or_else(|| "no month is set for this rule".to_string())
            } else {
                Ok((start, end))
            }
        }
        "day_length" => Err("a day-length calculation is not available yet".to_string()),
        "gdd" => Err("weather data for growing degree days is not available yet".to_string()),
        other => Err(format!("unknown timing anchor {other:?}")),
    }
}

/// A perennial repeats care, not the events that establish it.
fn repeats_yearly(kind: &str) -> bool {
    !matches!(
        kind,
        "sow_indoor" | "sow_outdoor" | "transplant" | "succession" | "thinning"
    )
}

fn rule_matches_start(rule: &TimingRule, planting: &Planting) -> bool {
    match planting.start_kind.as_str() {
        "transplanted" | "planted" => rule.kind == "transplant",
        "sown_indoor" => rule.kind == "sow_indoor",
        "sown_outdoor" => rule.kind == "sow_outdoor",
        kind => rule.kind == kind,
    }
}

fn offset_window(base: Date, rule: &TimingRule) -> (Date, Date) {
    let first = rule.offset_min_days.unwrap_or(0);
    let second = rule.offset_max_days.unwrap_or(first);
    let (min, max) = if first <= second {
        (first, second)
    } else {
        (second, first)
    };

    (base.add_days(min), base.add_days(max))
}

fn calendar_window(rule: &TimingRule, year: i32) -> Option<(Date, Date)> {
    let start_month = rule.month_start?;
    let end_month = rule.month_end.unwrap_or(start_month);
    let start = Date::new(year, start_month as u32, 1)?;
    let end_year = if end_month < start_month {
        year + 1
    } else {
        year
    };
    let end = Date::new(end_year, end_month as u32, 1)?.end_of_month();

    Some((start.min(end), start.max(end)))
}

fn midpoint(start: Date, end: Date) -> Date {
    start.add_days(start.days_until(end) / 2)
}

pub fn kind_label(kind: &str) -> String {
    match kind {
        "sow_indoor" => "Sow indoors".to_string(),
        "sow_outdoor" => "Sow outdoors".to_string(),
        "transplant" => "Transplant".to_string(),
        "harvest" => "Harvest".to_string(),
        "prune" => "Prune".to_string(),
        "feed" => "Feed".to_string(),
        "support" => "Support".to_string(),
        "repot" => "Repot".to_string(),
        "winter_protect" => "Winter protection".to_string(),
        "flowering" => "Flowering".to_string(),
        "fruiting" => "Fruiting".to_string(),
        "dormancy" => "Dormancy".to_string(),
        "succession" => "Succession sowing".to_string(),
        "thinning" => "Thinning".to_string(),
        "deadheading" => "Deadheading".to_string(),
        "bloom" => "Bloom".to_string(),
        other => other.replace('_', " "),
    }
}

pub fn start_kind_label(kind: &str) -> &'static str {
    match kind {
        "planted" => "Planted",
        "sown_indoor" => "Sown indoors",
        "sown_outdoor" => "Sown outdoors",
        "transplanted" => "Transplanted",
        _ => "Started",
    }
}

#[cfg(test)]
mod tests {
    use super::{StageWindow, kind_label};
    use crate::calendar::Date;
    use crate::frost::FrostDates;
    use crate::plant_data::{Stage, TimingRule};
    use crate::planting::Planting;

    fn date(year: i32, month: u32, day: u32) -> Date {
        Date::new(year, month, day).unwrap()
    }

    fn planting(started_on: Date, start_kind: &str) -> Planting {
        Planting {
            id: 1,
            profile_id: 1,
            space_id: 1,
            name: None,
            quantity: 1,
            container: "ground".into(),
            container_volume_l: None,
            substrate: None,
            started_on,
            start_kind: start_kind.into(),
            ended_on: None,
            outcome: None,
            notes: None,
        }
    }

    fn stage(
        id: i64,
        position: i64,
        kind: &str,
        label: &str,
        min: Option<f64>,
        max: Option<f64>,
    ) -> Stage {
        Stage {
            id,
            profile_id: 1,
            position,
            kind: kind.into(),
            label: label.into(),
            duration_days_min: min,
            duration_days_max: max,
            notes: None,
            repeats: None,
        }
    }

    fn rule(
        kind: &str,
        anchor: &str,
        stage_id: Option<i64>,
        min: Option<i64>,
        max: Option<i64>,
    ) -> TimingRule {
        TimingRule {
            id: 1,
            profile_id: 1,
            stage_id,
            kind: kind.into(),
            anchor: anchor.into(),
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

    fn build_without_starts(
        planting: &Planting,
        stages: &[Stage],
        rules: &[TimingRule],
        frost: &FrostDates,
        today: Date,
    ) -> super::Schedule {
        super::build(planting, stages, rules, &[], frost, today, None)
    }

    fn annual() -> Vec<Stage> {
        vec![
            stage(1, 1, "germination", "Germination", Some(10.0), Some(14.0)),
            stage(2, 2, "vegetative", "Vegetative", Some(20.0), Some(20.0)),
            stage(3, 3, "harvest", "Harvest", Some(30.0), Some(40.0)),
        ]
    }

    #[test]
    fn places_stages_with_their_uncertainty() {
        let planting = planting(date(2027, 5, 1), "sown_outdoor");
        let schedule = build_without_starts(
            &planting,
            &annual(),
            &[],
            &FrostDates::default(),
            date(2027, 5, 10),
        );

        assert_eq!(schedule.stages.len(), 3);
        assert_eq!(schedule.stages[0].likely_start, date(2027, 5, 1));
        assert_eq!(schedule.stages[0].likely_end, date(2027, 5, 13));
        assert_eq!(schedule.stages[1].earliest_start, date(2027, 5, 11));
        assert_eq!(schedule.stages[1].latest_start, date(2027, 5, 15));
        assert_eq!(schedule.stages[2].likely_start, date(2027, 6, 2));
        assert_eq!(schedule.stages[2].likely_end, date(2027, 7, 7));
        assert_eq!(schedule.span_end, date(2027, 7, 14));
        assert_eq!(schedule.current_stage, Some(0));
        assert_eq!(schedule.milestones[0].label, "Sown outdoors");
    }

    #[test]
    fn the_later_stage_wins_on_a_boundary_day() {
        let planting = planting(date(2027, 5, 1), "sown_outdoor");
        let schedule = build_without_starts(
            &planting,
            &annual(),
            &[],
            &FrostDates::default(),
            date(2027, 5, 13),
        );

        assert_eq!(schedule.current_stage, Some(1));
        assert_eq!(
            schedule.stage_on(date(2027, 5, 13)).unwrap().stage.label,
            "Vegetative"
        );
    }

    #[test]
    fn a_logged_stage_start_replaces_the_estimate() {
        let planting = planting(date(2027, 5, 1), "sown_outdoor");
        let stages = annual();
        let stage_starts = [(2_i64, date(2027, 5, 20))];

        let schedule = super::build(
            &planting,
            &stages,
            &[],
            &stage_starts,
            &FrostDates::default(),
            date(2027, 5, 20),
            None,
        );

        assert!(schedule.stages[1].actual_start);
        assert_eq!(schedule.stages[1].likely_start, date(2027, 5, 20));
        assert_eq!(schedule.stages[1].likely_end, date(2027, 6, 9));
        assert_eq!(schedule.stages[0].likely_end, date(2027, 5, 20));
        assert_eq!(schedule.stages[2].likely_start, date(2027, 6, 9));
        assert_eq!(schedule.current_stage, Some(1));
        assert!(
            schedule
                .milestones
                .iter()
                .any(|milestone| milestone.label == "Vegetative started")
        );
    }

    #[test]
    fn a_logged_stage_without_a_duration_is_a_point() {
        let planting = planting(date(2027, 5, 1), "sown_outdoor");
        let stages = vec![
            stage(1, 1, "germination", "Germination", Some(10.0), Some(14.0)),
            stage(2, 2, "vegetative", "Vegetative", None, None),
        ];
        let stage_starts = [(2_i64, date(2027, 5, 20))];

        let schedule = super::build(
            &planting,
            &stages,
            &[],
            &stage_starts,
            &FrostDates::default(),
            date(2027, 5, 20),
            None,
        );

        assert_eq!(schedule.stages.len(), 2);
        assert!(schedule.stages[1].actual_start);
        assert_eq!(schedule.stages[1].likely_start, date(2027, 5, 20));
        assert_eq!(schedule.stages[1].likely_end, date(2027, 5, 20));
        assert!(schedule.undated_stages.is_empty());
        assert!(
            schedule
                .unknown
                .iter()
                .any(|note| note.contains("only the started date"))
        );
    }

    #[test]
    fn an_out_of_order_stage_start_keeps_the_estimate() {
        let planting = planting(date(2027, 5, 1), "sown_outdoor");
        let stages = annual();
        let stage_starts = [(3_i64, date(2027, 5, 5))];

        let schedule = super::build(
            &planting,
            &stages,
            &[],
            &stage_starts,
            &FrostDates::default(),
            date(2027, 5, 5),
            None,
        );

        assert!(!schedule.stages[2].actual_start);
        assert!(
            schedule
                .unknown
                .iter()
                .any(|note| note.contains("before the previous stage"))
        );
    }

    #[test]
    fn a_transplant_skips_germination_and_seedling() {
        let planting = planting(date(2027, 5, 20), "transplanted");
        let stages = vec![
            stage(1, 1, "germination", "Germination", Some(10.0), Some(14.0)),
            stage(2, 2, "seedling", "Seedling", Some(20.0), Some(25.0)),
            stage(3, 3, "vegetative", "Vegetative", Some(30.0), Some(30.0)),
        ];
        let schedule = build_without_starts(
            &planting,
            &stages,
            &[],
            &FrostDates::default(),
            date(2027, 5, 20),
        );

        assert_eq!(schedule.stages.len(), 1);
        assert_eq!(schedule.stages[0].stage.label, "Vegetative");
        assert_eq!(schedule.stages[0].likely_start, date(2027, 5, 20));
        assert_eq!(schedule.current_stage, Some(0));
    }

    #[test]
    fn a_missing_duration_stops_the_timeline() {
        let planting = planting(date(2027, 5, 1), "sown_outdoor");
        let stages = vec![
            stage(1, 1, "germination", "Germination", Some(10.0), Some(14.0)),
            stage(2, 2, "vegetative", "Vegetative", None, None),
            stage(3, 3, "harvest", "Harvest", Some(30.0), Some(40.0)),
        ];
        let schedule = build_without_starts(
            &planting,
            &stages,
            &[],
            &FrostDates::default(),
            date(2027, 5, 5),
        );

        assert_eq!(schedule.stages.len(), 1);
        assert!(
            schedule
                .undated_stages
                .iter()
                .any(|label| label == "Vegetative")
        );
    }

    #[test]
    fn resolves_sowing_transplant_and_stage_rules() {
        let planting = planting(date(2027, 5, 1), "sown_outdoor");
        let stages = annual();
        let rules = vec![
            {
                let mut rule = rule("harvest", "sowing", None, Some(80), Some(100));
                rule.id = 1;
                rule
            },
            {
                let mut rule = rule("feed", "stage_start", Some(2), Some(0), Some(0));
                rule.id = 2;
                rule
            },
            {
                let mut rule = rule("transplant", "sowing", None, Some(20), Some(20));
                rule.id = 3;
                rule
            },
        ];
        let schedule = build_without_starts(
            &planting,
            &stages,
            &rules,
            &FrostDates::default(),
            date(2027, 5, 1),
        );

        let harvest = schedule
            .milestones
            .iter()
            .find(|milestone| milestone.kind == "harvest")
            .unwrap();
        assert_eq!(harvest.date, date(2027, 7, 20));
        assert_eq!(harvest.end, Some(date(2027, 8, 9)));

        let feed = schedule
            .milestones
            .iter()
            .find(|milestone| milestone.kind == "feed")
            .unwrap();
        assert_eq!(feed.date, schedule.stages[1].likely_start);

        let transplant = schedule
            .milestones
            .iter()
            .find(|milestone| milestone.kind == "transplant")
            .unwrap();
        assert_eq!(transplant.date, date(2027, 5, 21));
    }

    #[test]
    fn resolves_frost_and_calendar_rules() {
        let planting = planting(date(2027, 5, 1), "sown_outdoor");
        let frost = FrostDates {
            last: Some((4, 15)),
            first: None,
        };
        let mut sow = rule("sow_indoor", "last_frost", None, Some(-70), Some(-56));
        sow.id = 1;
        let mut prune = rule("prune", "calendar", None, None, None);
        prune.id = 2;
        prune.month_start = Some(3);
        prune.month_end = Some(3);

        let schedule = build_without_starts(
            &planting,
            &annual(),
            &[sow, prune],
            &frost,
            date(2027, 5, 1),
        );

        let sow = schedule
            .milestones
            .iter()
            .find(|milestone| milestone.kind == "sow_indoor")
            .unwrap();
        assert_eq!(sow.date, date(2027, 2, 4));
        assert_eq!(sow.end, Some(date(2027, 2, 18)));

        let prune = schedule
            .milestones
            .iter()
            .find(|milestone| milestone.kind == "prune")
            .unwrap();
        assert_eq!(prune.date, date(2028, 3, 1));
        assert_eq!(prune.end, Some(date(2028, 3, 31)));
    }

    #[test]
    fn a_transplant_uses_the_sowing_rule_for_sowing_anchors() {
        let planting = planting(date(2027, 5, 20), "transplanted");
        let stages = vec![stage(
            1,
            1,
            "vegetative",
            "Vegetative",
            Some(30.0),
            Some(30.0),
        )];
        let mut sow = rule("sow_indoor", "last_frost", None, Some(-70), Some(-56));
        sow.id = 1;
        let mut harvest = rule("harvest", "sowing", None, Some(80), Some(80));
        harvest.id = 2;
        let frost = FrostDates {
            last: Some((4, 15)),
            first: None,
        };

        let schedule = build_without_starts(
            &planting,
            &stages,
            &[sow, harvest],
            &frost,
            date(2027, 5, 20),
        );

        assert!(
            schedule
                .milestones
                .iter()
                .any(|milestone| milestone.kind == "sow_indoor")
        );
        let harvest = schedule
            .milestones
            .iter()
            .find(|milestone| milestone.kind == "harvest")
            .unwrap();
        // The sowing window is 4–18 February; its midpoint is 11 February.
        assert_eq!(harvest.date, date(2027, 5, 2));
    }

    #[test]
    fn a_sow_rule_can_be_anchored_to_the_transplant() {
        let planting = planting(date(2027, 5, 20), "transplanted");
        let stages = vec![stage(
            1,
            1,
            "vegetative",
            "Vegetative",
            Some(30.0),
            Some(30.0),
        )];
        let mut sow = rule("sow_indoor", "transplant", None, Some(-42), Some(-42));
        sow.id = 1;
        let mut harvest = rule("harvest", "sowing", None, Some(80), Some(80));
        harvest.id = 2;

        let schedule = build_without_starts(
            &planting,
            &stages,
            &[sow, harvest],
            &FrostDates::default(),
            date(2027, 5, 20),
        );

        let sow = schedule
            .milestones
            .iter()
            .find(|milestone| milestone.kind == "sow_indoor")
            .unwrap();
        assert_eq!(sow.date, date(2027, 4, 8));

        let harvest = schedule
            .milestones
            .iter()
            .find(|milestone| milestone.kind == "harvest")
            .unwrap();
        assert_eq!(harvest.date, date(2027, 6, 27));
    }

    #[test]
    fn a_transplant_without_a_sow_rule_does_not_guess_the_sowing_date() {
        let planting = planting(date(2027, 5, 20), "transplanted");
        let stages = vec![stage(
            1,
            1,
            "vegetative",
            "Vegetative",
            Some(30.0),
            Some(30.0),
        )];
        let mut harvest = rule("harvest", "sowing", None, Some(80), Some(80));
        harvest.id = 1;

        let schedule = build_without_starts(
            &planting,
            &stages,
            &[harvest],
            &FrostDates::default(),
            date(2027, 5, 20),
        );

        assert!(
            schedule
                .unknown
                .iter()
                .any(|note| note.contains("sowing date is unknown"))
        );
        assert!(
            !schedule
                .milestones
                .iter()
                .any(|milestone| milestone.kind == "harvest")
        );
    }

    #[test]
    fn a_rule_that_repeats_the_start_is_not_added_twice() {
        let planting = planting(date(2027, 5, 1), "sown_outdoor");
        let stages = annual();
        let mut sow = rule("sow_outdoor", "sowing", None, Some(0), Some(0));
        sow.id = 1;

        let schedule = build_without_starts(
            &planting,
            &stages,
            &[sow],
            &FrostDates::default(),
            date(2027, 5, 1),
        );

        assert_eq!(
            schedule
                .milestones
                .iter()
                .filter(|milestone| milestone.kind == "sown_outdoor")
                .count(),
            1
        );
        assert!(
            !schedule
                .milestones
                .iter()
                .any(|milestone| milestone.kind == "sow_outdoor")
        );
    }

    #[test]
    fn a_later_transplant_rule_is_not_hidden() {
        let planting = planting(date(2027, 5, 1), "planted");
        let stages = annual();
        let mut later = rule("transplant", "transplant", None, Some(30), Some(30));
        later.id = 1;

        let schedule = build_without_starts(
            &planting,
            &stages,
            &[later],
            &FrostDates::default(),
            date(2027, 5, 1),
        );

        let milestone = schedule
            .milestones
            .iter()
            .find(|milestone| milestone.kind == "transplant")
            .unwrap();
        assert_eq!(milestone.date, date(2027, 5, 31));
    }

    #[test]
    fn a_transplant_profile_without_a_later_stage_is_reported() {
        let planting = planting(date(2027, 5, 20), "transplanted");
        let stages = vec![
            stage(1, 1, "germination", "Germination", Some(10.0), Some(14.0)),
            stage(2, 2, "seedling", "Seedling", Some(14.0), Some(21.0)),
        ];

        let schedule = build_without_starts(
            &planting,
            &stages,
            &[],
            &FrostDates::default(),
            date(2027, 5, 20),
        );

        assert!(schedule.stages.is_empty());
        assert!(
            schedule
                .unknown
                .iter()
                .any(|note| note.contains("no stage after germination"))
        );
    }

    #[test]
    fn reports_anchors_that_need_more_data() {
        let planting = planting(date(2027, 5, 1), "sown_outdoor");
        let mut gdd = rule("harvest", "gdd", None, Some(0), Some(0));
        gdd.id = 1;
        let schedule = build_without_starts(
            &planting,
            &annual(),
            &[gdd],
            &FrostDates::default(),
            date(2027, 5, 1),
        );

        assert!(
            schedule
                .unknown
                .iter()
                .any(|note| note.contains("weather data"))
        );
    }

    #[test]
    fn picks_the_next_milestone_and_the_stage_on_a_date() {
        let planting = planting(date(2027, 5, 1), "sown_outdoor");
        let mut harvest = rule("harvest", "sowing", None, Some(80), Some(100));
        harvest.id = 1;
        let schedule = build_without_starts(
            &planting,
            &annual(),
            &[harvest],
            &FrostDates::default(),
            date(2027, 6, 1),
        );

        assert_eq!(schedule.current_stage, Some(1));
        assert!(schedule.next.is_some());
        assert!(schedule.stage_on(date(2027, 5, 5)).is_some());
        assert!(schedule.stage_on(date(2027, 12, 1)).is_none());
    }

    #[test]
    fn labels_unknown_kinds_readably() {
        assert_eq!(kind_label("repot"), "Repot");
        assert_eq!(kind_label("custom_care"), "custom care");
    }

    fn perennial() -> Vec<Stage> {
        vec![
            stage(1, 1, "germination", "Germination", Some(10.0), Some(14.0)),
            stage(2, 2, "vegetative", "Vegetative", Some(100.0), Some(100.0)),
            stage(3, 3, "flowering", "Flowering", Some(50.0), Some(50.0)),
            stage(4, 4, "dormancy", "Dormancy", Some(200.0), Some(200.0)),
        ]
    }

    #[test]
    fn a_logged_start_in_the_second_cycle_leaves_the_first_alone() {
        let planting = planting(date(2025, 1, 1), "sown_outdoor");
        let plan = super::build(
            &planting,
            &perennial(),
            &[],
            &[],
            &FrostDates::default(),
            date(2027, 1, 1),
            Some("perennial"),
        );
        let dormancies = || {
            plan.stages
                .iter()
                .filter(|window| window.stage.kind == "dormancy")
        };
        let first = dormancies().next().expect("a first dormancy").likely_start;
        let second = dormancies().nth(1).expect("a second dormancy").likely_start;

        // The second winter starts a few days later than planned.
        let logged = second.add_days(4);
        let schedule = super::build(
            &planting,
            &perennial(),
            &[],
            &[(4, logged)],
            &FrostDates::default(),
            date(2027, 1, 1),
            Some("perennial"),
        );
        let dormancies: Vec<_> = schedule
            .stages
            .iter()
            .filter(|window| window.stage.kind == "dormancy")
            .collect();

        assert_eq!(dormancies[0].likely_start, first, "the first cycle stays");
        assert!(!dormancies[0].actual_start);
        assert_eq!(dormancies[1].likely_start, logged);
        assert!(dormancies[1].actual_start);
    }

    #[test]
    fn a_logged_start_moves_the_cycle_it_belongs_to() {
        let planting = planting(date(2025, 1, 1), "sown_outdoor");
        let plan = super::build(
            &planting,
            &perennial(),
            &[],
            &[],
            &FrostDates::default(),
            date(2027, 1, 1),
            Some("perennial"),
        );
        let first = plan
            .stages
            .iter()
            .find(|window| window.stage.kind == "dormancy")
            .expect("a dormancy")
            .likely_start;
        let logged = first.add_days(10);

        let schedule = super::build(
            &planting,
            &perennial(),
            &[],
            &[(4, logged)],
            &FrostDates::default(),
            date(2027, 1, 1),
            Some("perennial"),
        );

        let dormancy = schedule
            .stages
            .iter()
            .find(|window| window.stage.kind == "dormancy")
            .expect("a dormancy");
        assert_eq!(dormancy.likely_start, logged);
        assert!(dormancy.actual_start);

        // The previous stage ends on the logged day; the new stage takes that day over.
        let flowering = schedule
            .stages
            .iter()
            .find(|window| window.stage.kind == "flowering")
            .expect("a flowering");
        assert_eq!(flowering.likely_end, logged);
    }

    #[test]
    fn a_second_date_for_the_same_cycle_does_not_move_the_next_one() {
        let planting = planting(date(2025, 1, 1), "sown_outdoor");
        let plan = super::build(
            &planting,
            &perennial(),
            &[],
            &[],
            &FrostDates::default(),
            date(2027, 1, 1),
            Some("perennial"),
        );
        let starts: Vec<_> = plan
            .stages
            .iter()
            .filter(|window| window.stage.kind == "dormancy")
            .map(|window| window.likely_start)
            .collect();
        let next_cycle = starts[1];

        // Two entries for the first winter; the first one wins and the second cycle stays.
        let schedule = super::build(
            &planting,
            &perennial(),
            &[],
            &[(4, starts[0].add_days(2)), (4, starts[0].add_days(5))],
            &FrostDates::default(),
            date(2027, 1, 1),
            Some("perennial"),
        );
        let dormancies: Vec<_> = schedule
            .stages
            .iter()
            .filter(|window| window.stage.kind == "dormancy")
            .collect();

        assert_eq!(dormancies[0].likely_start, starts[0].add_days(2));
        assert_eq!(dormancies[1].likely_start, next_cycle);
        assert!(!dormancies[1].actual_start);
    }

    #[test]
    fn a_perennial_repeats_its_cycle_after_the_first_year() {
        let planting = planting(date(2025, 1, 1), "sown_outdoor");
        let schedule = super::build(
            &planting,
            &perennial(),
            &[],
            &[],
            &FrostDates::default(),
            date(2027, 1, 1),
            Some("perennial"),
        );

        // Germination and seedling happen once; the rest returns every cycle.
        let germinations = schedule
            .stages
            .iter()
            .filter(|window| window.stage.kind == "germination")
            .count();
        assert_eq!(germinations, 1);
        assert!(
            schedule.stages.len() > perennial().len(),
            "the cycle should repeat"
        );

        let dormancies = schedule
            .stages
            .iter()
            .filter(|window| window.stage.kind == "dormancy")
            .count();
        assert!(dormancies >= 2);

        // The repeated cycle starts on the anniversary of the first cycle, not immediately after
        // the previous one, so a short cycle leaves a gap instead of drifting out of season.
        let first_vegetative = schedule
            .stages
            .iter()
            .find(|window| window.stage.kind == "vegetative")
            .unwrap();
        let second_vegetative = schedule
            .stages
            .iter()
            .filter(|window| window.stage.kind == "vegetative")
            .nth(1)
            .unwrap();
        assert_eq!(second_vegetative.likely_start, date(2026, 1, 13));
        assert!(first_vegetative.likely_start < second_vegetative.likely_start);

        // The timeline covers today and one season ahead.
        assert!(schedule.span_end >= date(2027, 1, 1));
        assert!(schedule.stage_on(date(2026, 6, 1)).is_some());
        // 1 January falls in the honest gap between two cycles.
        assert!(schedule.stage_on(date(2027, 1, 1)).is_none());
    }

    #[test]
    fn a_perennial_repeats_only_the_marked_stages() {
        let mut stages = vec![
            stage(
                1,
                1,
                "establishment",
                "Establishment",
                Some(20.0),
                Some(20.0),
            ),
            stage(2, 2, "vegetative", "Vegetative", Some(60.0), Some(60.0)),
            stage(3, 3, "harvest", "Harvest", Some(200.0), Some(200.0)),
        ];
        // A houseplant has no yearly vegetative cycle: only the harvest returns.
        stages[1].repeats = Some(false);
        stages[2].repeats = Some(true);

        let planting = planting(date(2025, 1, 1), "planted");
        let schedule = super::build(
            &planting,
            &stages,
            &[],
            &[],
            &FrostDates::default(),
            date(2027, 1, 1),
            Some("perennial"),
        );

        assert_eq!(
            schedule
                .stages
                .iter()
                .filter(|window| window.stage.kind == "vegetative")
                .count(),
            1,
            "an unmarked stage happens once"
        );
        let harvests: Vec<_> = schedule
            .stages
            .iter()
            .filter(|window| window.stage.kind == "harvest")
            .collect();
        assert!(harvests.len() >= 2);
        assert_eq!(harvests[1].likely_start, date(2026, 3, 22));
    }

    #[test]
    fn an_overlong_perennial_cycle_keeps_the_timeline_in_date_order() {
        let stages = vec![
            stage(1, 1, "vegetative", "Vegetative", Some(400.0), Some(400.0)),
            stage(2, 2, "flowering", "Flowering", Some(30.0), Some(30.0)),
        ];
        let planting = planting(date(2024, 1, 1), "sown_outdoor");
        let schedule = super::build(
            &planting,
            &stages,
            &[],
            &[],
            &FrostDates::default(),
            date(2026, 1, 1),
            Some("perennial"),
        );

        let starts: Vec<_> = schedule
            .stages
            .iter()
            .map(|window| window.likely_start)
            .collect();
        assert!(
            starts.windows(2).all(|pair| pair[0] <= pair[1]),
            "stages should be in date order: {starts:?}"
        );

        let day = schedule.stage_on(date(2025, 2, 10)).unwrap();
        assert_eq!(day.stage.label, "Flowering");
    }

    #[test]
    fn a_single_mature_stage_continues_instead_of_restarting() {
        let mut stages = vec![
            stage(
                1,
                1,
                "establishment",
                "Establishment",
                Some(20.0),
                Some(20.0),
            ),
            stage(2, 2, "mature", "Mature", Some(200.0), Some(200.0)),
        ];
        stages[1].repeats = Some(true);

        let planting = planting(date(2025, 1, 1), "planted");
        let schedule = super::build(
            &planting,
            &stages,
            &[],
            &[],
            &FrostDates::default(),
            date(2026, 6, 1),
            Some("perennial"),
        );

        let matures: Vec<_> = schedule
            .stages
            .iter()
            .filter(|window| window.stage.kind == "mature")
            .collect();
        assert_eq!(
            matures.len(),
            1,
            "a mature state does not restart each year"
        );
        assert_eq!(matures[0].likely_start, date(2025, 1, 21));
        assert_eq!(matures[0].likely_end, date(2027, 6, 1));
        assert_eq!(
            schedule.stage_on(date(2026, 12, 1)).unwrap().stage.label,
            "Mature"
        );
    }

    #[test]
    fn a_perennial_without_marked_stages_does_not_repeat() {
        let mut stages = perennial();
        for stage in &mut stages {
            stage.repeats = Some(false);
        }

        let planting = planting(date(2025, 1, 1), "sown_outdoor");
        let schedule = super::build(
            &planting,
            &stages,
            &[],
            &[],
            &FrostDates::default(),
            date(2027, 1, 1),
            Some("perennial"),
        );

        assert_eq!(schedule.stages.len(), stages.len());
    }

    #[test]
    fn a_perennial_repeats_a_calendar_care_rule() {
        let planting = planting(date(2025, 1, 1), "sown_outdoor");
        let mut prune = rule("prune", "calendar", None, None, None);
        prune.month_start = Some(3);
        prune.month_end = Some(3);

        let schedule = super::build(
            &planting,
            &perennial(),
            &[prune],
            &[],
            &FrostDates::default(),
            date(2027, 1, 1),
            Some("perennial"),
        );

        let prunes: Vec<_> = schedule
            .milestones
            .iter()
            .filter(|milestone| milestone.kind == "prune")
            .collect();
        assert!(prunes.len() >= 3, "one March pruning per year");
        assert_eq!(prunes[0].date, date(2025, 3, 1));
        assert_eq!(prunes[1].date, date(2026, 3, 1));
        assert!(schedule.next.is_some());
    }

    #[test]
    fn a_perennial_does_not_repeat_a_sowing_rule() {
        let planting = planting(date(2025, 1, 1), "sown_outdoor");
        let mut sow = rule("sow_outdoor", "calendar", None, None, None);
        sow.month_start = Some(3);
        sow.month_end = Some(3);

        let schedule = super::build(
            &planting,
            &perennial(),
            &[sow],
            &[],
            &FrostDates::default(),
            date(2027, 1, 1),
            Some("perennial"),
        );

        assert_eq!(
            schedule
                .milestones
                .iter()
                .filter(|milestone| milestone.kind == "sow_outdoor")
                .count(),
            1
        );
    }

    #[test]
    fn an_annual_does_not_repeat() {
        let planting = planting(date(2025, 1, 1), "sown_outdoor");
        let schedule = super::build(
            &planting,
            &annual(),
            &[],
            &[],
            &FrostDates::default(),
            date(2027, 1, 1),
            Some("annual"),
        );

        assert_eq!(schedule.stages.len(), annual().len());
        assert!(schedule.span_end < date(2025, 12, 31));
    }

    #[test]
    fn a_finished_perennial_stops_at_its_end() {
        let mut planting = planting(date(2025, 1, 1), "sown_outdoor");
        planting.ended_on = Some(date(2025, 6, 1));

        let schedule = super::build(
            &planting,
            &perennial(),
            &[],
            &[],
            &FrostDates::default(),
            date(2027, 1, 1),
            Some("perennial"),
        );

        assert_eq!(
            schedule.stages.len(),
            3,
            "dormancy starts after the planting ended"
        );
        assert!(
            schedule
                .stages
                .iter()
                .all(|window| window.likely_end <= date(2025, 6, 1))
        );
        assert_eq!(schedule.span_end, date(2025, 6, 1));
    }

    #[test]
    fn a_finished_perennial_gets_no_care_after_its_end() {
        let mut planting = planting(date(2025, 1, 1), "sown_outdoor");
        planting.ended_on = Some(date(2025, 6, 1));

        let mut spring = rule("prune", "calendar", None, None, None);
        spring.month_start = Some(3);
        spring.month_end = Some(3);
        let mut autumn = rule("harvest", "calendar", None, None, None);
        autumn.month_start = Some(9);
        autumn.month_end = Some(9);

        let schedule = super::build(
            &planting,
            &perennial(),
            &[spring, autumn],
            &[],
            &FrostDates::default(),
            date(2027, 1, 1),
            Some("perennial"),
        );

        assert_eq!(
            schedule
                .milestones
                .iter()
                .filter(|milestone| milestone.kind == "prune")
                .count(),
            1
        );
        assert_eq!(
            schedule
                .milestones
                .iter()
                .filter(|milestone| milestone.kind == "harvest")
                .count(),
            0
        );
    }

    #[test]
    fn a_window_can_be_checked_for_a_date() {
        let window = StageWindow {
            stage: stage(1, 1, "vegetative", "Vegetative", Some(1.0), Some(1.0)),
            earliest_start: date(2027, 5, 1),
            latest_start: date(2027, 5, 2),
            earliest_end: date(2027, 5, 3),
            latest_end: date(2027, 5, 4),
            likely_start: date(2027, 5, 1),
            likely_end: date(2027, 5, 3),
            actual_start: false,
        };

        assert!(window.contains_likely(date(2027, 5, 2)));
        assert!(!window.contains_likely(date(2027, 5, 4)));
    }
}
