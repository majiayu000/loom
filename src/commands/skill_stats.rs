use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, Utc};
use serde_json::{Value, json};

use crate::cli::SkillStatsArgs;
use crate::envelope::Meta;
use crate::state_model::{RegistrySnapshot, RegistryStatePaths};
use crate::types::ErrorCode;

use super::helpers::{map_lock, map_registry_state, validate_skill_name};
use super::skill_inventory::build_skill_read_model_from_snapshot;
use super::telemetry::{
    AgentRef, SkillRef, UsageKind, UsageRow, load_dataset, parse_cutoff, usage_rows,
};
use super::{App, CommandFailure};

const ERROR_RATE_MIN_SAMPLE: u64 = 5;
const SINGLE_RUNTIME_SCOPE: &str = "all_agents";

#[derive(Clone, Default)]
struct AttemptAggregate {
    invocation_count: u64,
    error_count: u64,
    attempt_count: u64,
    last_used: Option<DateTime<Utc>>,
    failure_categories: BTreeMap<String, u64>,
}

impl AttemptAggregate {
    fn record(&mut self, row: &UsageRow, field: &str) -> Result<(), CommandFailure> {
        checked_stats_increment(&mut self.attempt_count, &format!("{field}.attempt_count"))?;
        match row.kind {
            UsageKind::Invocation => checked_stats_increment(
                &mut self.invocation_count,
                &format!("{field}.invocation_count"),
            )?,
            UsageKind::Error => {
                checked_stats_increment(&mut self.error_count, &format!("{field}.error_count"))?;
                let category = row.failure_category.as_deref().ok_or_else(|| {
                    corrupt(format!(
                        "{field}.failure_categories: skill.error row has no failure category"
                    ))
                })?;
                let count = self
                    .failure_categories
                    .entry(category.to_string())
                    .or_default();
                checked_stats_increment(count, &format!("{field}.failure_categories.{category}"))?;
            }
        }
        if self.last_used.is_none_or(|current| row.timestamp > current) {
            self.last_used = Some(row.timestamp);
        }
        Ok(())
    }

    fn json(&self, last_used_field: &str) -> Value {
        let mut value = json!({
            "invocation_count": self.invocation_count,
            "error_count": self.error_count,
            "attempt_count": self.attempt_count,
            "error_rate": (self.attempt_count >= ERROR_RATE_MIN_SAMPLE)
                .then(|| self.error_count as f64 / self.attempt_count as f64),
            "error_sample_size": self.attempt_count,
            "failure_categories": self.failure_categories,
        });
        value[last_used_field] = json!(self.last_used.map(|timestamp| timestamp.to_rfc3339()));
        value
    }
}

#[derive(Clone, Default)]
struct WindowAggregate {
    total: AttemptAggregate,
    by_agent: BTreeMap<String, AttemptAggregate>,
}

impl WindowAggregate {
    fn record(&mut self, row: &UsageRow, field: &str) -> Result<(), CommandFailure> {
        self.total.record(row, field)?;
        if let AgentRef::Known(agent) = &row.agent_ref {
            self.by_agent
                .entry(agent.clone())
                .or_default()
                .record(row, &format!("{field}.by_agent.{agent}"))?;
        }
        Ok(())
    }

    fn by_agent_json(&self) -> Value {
        Value::Object(
            self.by_agent
                .iter()
                .map(|(agent, aggregate)| (agent.clone(), aggregate.json("window_last_used")))
                .collect(),
        )
    }
}

#[derive(Clone, Default)]
struct LifetimeAggregate {
    attempt_count: u64,
    last_used: Option<DateTime<Utc>>,
}

impl LifetimeAggregate {
    fn record(&mut self, row: &UsageRow, field: &str) -> Result<(), CommandFailure> {
        checked_stats_increment(&mut self.attempt_count, field)?;
        if self.last_used.is_none_or(|current| row.timestamp > current) {
            self.last_used = Some(row.timestamp);
        }
        Ok(())
    }
}

struct SkillRecord {
    skill: String,
    category: &'static str,
    single_runtime: bool,
    last_used: Option<DateTime<Utc>>,
    window: WindowAggregate,
}

impl SkillRecord {
    fn json(&self) -> Value {
        let mut value = self.window.total.json("window_last_used");
        value["skill"] = json!(self.skill);
        value["category"] = json!(self.category);
        value["single_runtime"] = json!(self.single_runtime);
        value["last_used"] = json!(self.last_used.map(|timestamp| timestamp.to_rfc3339()));
        value["by_agent"] = self.window.by_agent_json();
        value
    }
}

impl App {
    pub(crate) fn cmd_skill_stats(
        &self,
        args: &SkillStatsArgs,
    ) -> Result<(Value, Meta), CommandFailure> {
        let since = parse_cutoff("--since", args.since.as_deref())?;
        let zombie_days = i64::try_from(args.zombie_days).map_err(|_| {
            CommandFailure::new(
                ErrorCode::ArgInvalid,
                "--zombie-days is outside the supported range",
            )
        })?;
        let zombie_duration = Duration::try_days(zombie_days).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::ArgInvalid,
                "--zombie-days is outside the supported range",
            )
        })?;
        let cutoff = Utc::now()
            .checked_sub_signed(zombie_duration)
            .ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    "--zombie-days produces an unsupported cutoff",
                )
            })?;

        let (inventory, dataset, bound_agents) = {
            let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
            let paths = RegistryStatePaths::from_app_context(&self.ctx);
            let snapshot = paths
                .maybe_load_snapshot()
                .map_err(map_registry_state)?
                .ok_or_else(|| {
                    CommandFailure::new(
                        ErrorCode::StateNotInitialized,
                        format!(
                            "registry state not initialized under {}",
                            paths.registry_dir.display()
                        ),
                    )
                })?;
            let inventory = build_skill_read_model_from_snapshot(&self.ctx, Some(&snapshot))
                .map_err(map_registry_state)?;
            let bound_agents = current_bound_agents(&snapshot)?;
            let dataset = load_dataset(&self.ctx)?;
            (inventory, dataset, bound_agents)
        };

        let skill_ids = inventory
            .skills
            .iter()
            .filter(|skill| {
                skill["sources"].as_array().is_some_and(|sources| {
                    sources.iter().any(|source| {
                        matches!(source.as_str(), Some("source" | "rule" | "projection"))
                    })
                })
            })
            .filter_map(|skill| skill["skill_id"].as_str())
            .filter(|skill| validate_skill_name(skill).is_ok())
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        let report = aggregate_stats(
            &skill_ids,
            &bound_agents,
            &dataset,
            args.agent.as_deref(),
            since,
            cutoff,
            args.zombie_days,
        )?;
        let mut warnings = inventory.warnings;
        if dataset.malformed_event_count > 0 {
            warnings.push(format!(
                "ignored {} malformed telemetry event record(s)",
                dataset.malformed_event_count
            ));
        }
        Ok((
            report,
            Meta {
                warnings,
                ..Meta::default()
            },
        ))
    }
}

fn current_bound_agents(
    snapshot: &RegistrySnapshot,
) -> Result<BTreeMap<String, BTreeSet<String>>, CommandFailure> {
    let bindings = snapshot
        .bindings
        .bindings
        .iter()
        .map(|binding| (binding.binding_id.as_str(), binding))
        .collect::<BTreeMap<_, _>>();
    let targets = snapshot
        .targets
        .targets
        .iter()
        .map(|target| (target.target_id.as_str(), target))
        .collect::<BTreeMap<_, _>>();
    let mut result = BTreeMap::<String, BTreeSet<String>>::new();
    for rule in &snapshot.rules.rules {
        let binding = bindings.get(rule.binding_id.as_str()).ok_or_else(|| {
            corrupt(format!(
                "skill_stats.bindings: rule for '{}' references missing binding '{}'",
                rule.skill_id, rule.binding_id
            ))
        })?;
        if !binding.active {
            continue;
        }
        let Some(target) = targets.get(rule.target_id.as_str()) else {
            return Err(corrupt(format!(
                "skill_stats.bindings: rule for '{}' references missing target '{}'",
                rule.skill_id, rule.target_id
            )));
        };
        if binding.agent != target.agent {
            return Err(corrupt(format!(
                "skill_stats.bindings: binding '{}' and target '{}' have different agents",
                binding.binding_id, target.target_id
            )));
        }
        result
            .entry(rule.skill_id.clone())
            .or_default()
            .insert(binding.agent.to_string());
    }
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn aggregate_stats(
    skill_ids: &BTreeSet<String>,
    bound_agents: &BTreeMap<String, BTreeSet<String>>,
    dataset: &super::telemetry::NormalizedTelemetryDataset,
    agent_filter: Option<&str>,
    since: Option<DateTime<Utc>>,
    zombie_cutoff: DateTime<Utc>,
    zombie_days: u64,
) -> Result<Value, CommandFailure> {
    let mut used_agents = BTreeMap::<String, BTreeSet<String>>::new();
    let mut scoped_lifetime = BTreeMap::<String, LifetimeAggregate>::new();
    let mut window_skills = BTreeMap::<String, WindowAggregate>::new();
    let mut orphans = BTreeMap::<(String, Option<String>), AttemptAggregate>::new();
    let mut unattributed = WindowAggregate::default();
    let mut agentless = AttemptAggregate::default();
    let mut window_events = 0_u64;

    for row in usage_rows(dataset) {
        let registered = match &row.skill_ref {
            SkillRef::Registered(skill) if skill_ids.contains(skill) => Some(skill.as_str()),
            _ => None,
        };
        if let (Some(skill), AgentRef::Known(agent)) = (registered, &row.agent_ref) {
            used_agents
                .entry(skill.to_string())
                .or_default()
                .insert(agent.clone());
        }

        let in_scope = agent_filter.is_none_or(
            |selected| matches!(&row.agent_ref, AgentRef::Known(agent) if agent == selected),
        );
        if in_scope {
            if let Some(skill) = registered {
                scoped_lifetime
                    .entry(skill.to_string())
                    .or_default()
                    .record(&row, &format!("scoped_lifetime.{skill}.attempt_count"))?;
            }
        } else {
            continue;
        }
        if since.is_some_and(|cutoff| row.timestamp < cutoff) {
            continue;
        }

        checked_stats_increment(&mut window_events, "window_events")?;
        if matches!(row.agent_ref, AgentRef::Unknown) {
            agentless.record(&row, "agentless")?;
        }
        match &row.skill_ref {
            SkillRef::Registered(skill) if skill_ids.contains(skill) => {
                window_skills
                    .entry(skill.clone())
                    .or_default()
                    .record(&row, &format!("skills.{skill}"))?;
            }
            SkillRef::Registered(skill) | SkillRef::Observed(skill)
                if validate_skill_name(skill).is_ok() =>
            {
                let agent = match &row.agent_ref {
                    AgentRef::Known(agent) => Some(agent.clone()),
                    AgentRef::Unknown => None,
                };
                orphans
                    .entry((skill.clone(), agent))
                    .or_default()
                    .record(&row, &format!("orphans.{skill}"))?;
            }
            SkillRef::Registered(_) | SkillRef::Observed(_) | SkillRef::Unattributed => {
                unattributed.record(&row, "unattributed")?;
            }
        }
    }

    let mut skills = skill_ids
        .iter()
        .map(|skill| {
            let all_bindings = bound_agents.get(skill).cloned().unwrap_or_default();
            let scoped_bindings = all_bindings
                .iter()
                .filter(|agent| agent_filter.is_none_or(|selected| *agent == selected))
                .cloned()
                .collect::<BTreeSet<_>>();
            let lifetime = scoped_lifetime.get(skill).cloned().unwrap_or_default();
            let category = if !scoped_bindings.is_empty() {
                if lifetime
                    .last_used
                    .is_some_and(|last_used| last_used >= zombie_cutoff)
                {
                    "active"
                } else {
                    "zombie"
                }
            } else if lifetime.attempt_count > 0 {
                "unbound_but_used"
            } else {
                "unbound_unused"
            };
            let global_used_agents = used_agents.get(skill).cloned().unwrap_or_default();
            SkillRecord {
                skill: skill.clone(),
                category,
                single_runtime: all_bindings.len() >= 2 && global_used_agents.len() == 1,
                last_used: lifetime.last_used,
                window: window_skills.get(skill).cloned().unwrap_or_default(),
            }
        })
        .collect::<Vec<_>>();
    skills.sort_by(compare_skill_records);

    let zombie_names = skills
        .iter()
        .filter(|skill| skill.category == "zombie")
        .map(|skill| skill.skill.clone())
        .collect::<Vec<_>>();
    let unbound_unused_names = skills
        .iter()
        .filter(|skill| skill.category == "unbound_unused")
        .map(|skill| skill.skill.clone())
        .collect::<Vec<_>>();
    let orphan_values = orphans
        .iter()
        .map(|((name, agent), aggregate)| {
            let mut value = aggregate.json("window_last_used");
            value["name"] = json!(name);
            value["agent"] = json!(agent);
            value
        })
        .collect::<Vec<_>>();

    let unattributed_json = unattributed.total.json("window_last_used");
    Ok(json!({
        "since": since.map(|value| value.to_rfc3339()),
        "agent": agent_filter,
        "zombie_days": zombie_days,
        "telemetry_enabled": dataset.telemetry_enabled,
        "telemetry_empty": dataset.persisted_event_count == 0,
        "persisted_events": dataset.persisted_event_count,
        "malformed_events": dataset.malformed_event_count,
        "single_runtime_scope": SINGLE_RUNTIME_SCOPE,
        "window_events": window_events,
        "unattributed_window_events": unattributed.total.attempt_count,
        "unattributed": unattributed_json,
        "agentless": agentless.json("window_last_used"),
        "skills": skills.iter().map(SkillRecord::json).collect::<Vec<_>>(),
        "zombies": zombie_names,
        "unbound_unused": unbound_unused_names,
        "orphans": orphan_values,
    }))
}

fn compare_skill_records(left: &SkillRecord, right: &SkillRecord) -> Ordering {
    let group = |category: &str| match category {
        "active" | "unbound_but_used" => 0_u8,
        "zombie" => 1,
        _ => 2,
    };
    group(left.category)
        .cmp(&group(right.category))
        .then_with(|| match group(left.category) {
            0 => right
                .window
                .total
                .attempt_count
                .cmp(&left.window.total.attempt_count),
            1 => compare_optional_timestamp(left.last_used, right.last_used),
            _ => Ordering::Equal,
        })
        .then_with(|| left.skill.cmp(&right.skill))
}

fn compare_optional_timestamp(
    left: Option<DateTime<Utc>>,
    right: Option<DateTime<Utc>>,
) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn checked_stats_increment(value: &mut u64, field: &str) -> Result<(), CommandFailure> {
    *value = value
        .checked_add(1)
        .ok_or_else(|| corrupt(format!("aggregate overflow at {field}")))?;
    Ok(())
}

fn corrupt(message: impl Into<String>) -> CommandFailure {
    CommandFailure::new(ErrorCode::StateCorrupt, message)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn aggregation_overflow_fails_without_partial_output() {
        let mut aggregate = AttemptAggregate {
            attempt_count: u64::MAX,
            ..AttemptAggregate::default()
        };
        let row = UsageRow {
            skill_ref: SkillRef::Registered("demo".to_string()),
            agent_ref: AgentRef::Known("codex".to_string()),
            kind: UsageKind::Invocation,
            timestamp: Utc.with_ymd_and_hms(2026, 7, 18, 0, 0, 0).unwrap(),
            failure_category: None,
        };
        let error = aggregate.record(&row, "skills.demo").unwrap_err();
        assert_eq!(error.code, ErrorCode::StateCorrupt);
        assert!(error.message.contains("skills.demo.attempt_count"));
        assert_eq!(aggregate.invocation_count, 0);
    }
}
