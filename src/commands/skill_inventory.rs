use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{Value, json};

use crate::envelope::Meta;
use crate::gitops;
use crate::state::AppContext;
use crate::state_model::{
    RegistryProjectionTarget, RegistrySnapshot, RegistryStatePaths, RegistryTrustFile,
};
use crate::types::ErrorCode;

use super::helpers::{map_arg, map_registry_state, validate_skill_name};
use super::{App, CommandFailure, SkillLintMode, lint_skill_source};

#[derive(Debug, Clone)]
pub(crate) struct SkillInventoryReadModel {
    pub skills: Vec<Value>,
    pub warnings: Vec<String>,
    pub registry_available: bool,
}

#[derive(Debug, Default)]
struct SkillReadRow {
    skill_id: String,
    entrypoint: Option<PathBuf>,
    description: Option<String>,
    source_path: Option<PathBuf>,
    source_status: Option<&'static str>,
    sources: BTreeSet<&'static str>,
    binding_ids: BTreeSet<String>,
    target_ids: BTreeSet<String>,
    observed_target_ids: BTreeSet<String>,
    profile_ids: BTreeSet<String>,
    compatible_agents: BTreeSet<String>,
    compatible_targets: BTreeMap<String, Value>,
    workspace_matchers: BTreeMap<String, Value>,
    warnings: Vec<String>,
    projection_count: usize,
    latest_rev: Option<String>,
    latest_updated_at: Option<String>,
    release_tags: Vec<String>,
    snapshot_tags: Vec<String>,
    observed_imported: bool,
    trust: Option<String>,
    quarantined: bool,
}

impl SkillReadRow {
    fn new(skill_id: String) -> Self {
        Self {
            skill_id,
            ..Self::default()
        }
    }
}

impl App {
    pub fn cmd_skill_list(&self) -> std::result::Result<(Value, Meta), CommandFailure> {
        let model = build_skill_read_model(&self.ctx).map_err(map_inventory_error)?;
        Ok((skill_list_payload(&model), inventory_meta(model.warnings)))
    }
}

pub(crate) fn skill_brief_payload(
    ctx: &AppContext,
    skill_id: &str,
) -> std::result::Result<(Value, Meta), CommandFailure> {
    validate_skill_name(skill_id).map_err(map_arg)?;
    let model = build_skill_read_model(ctx).map_err(map_inventory_error)?;
    let Some(skill) = find_skill(&model.skills, skill_id).cloned() else {
        return Err(CommandFailure::new(
            ErrorCode::SkillNotFound,
            format!("skill '{}' not found", skill_id),
        ));
    };
    Ok((json!({ "skill": skill }), inventory_meta(model.warnings)))
}

pub(crate) fn build_skill_read_model(ctx: &AppContext) -> Result<SkillInventoryReadModel> {
    let mut warnings = Vec::new();
    let mut rows: BTreeMap<String, SkillReadRow> = BTreeMap::new();

    add_source_skill_rows(&ctx.skills_dir, &mut rows, &mut warnings)?;

    let paths = RegistryStatePaths::from_app_context(ctx);
    let snapshot = paths.maybe_load_snapshot()?;
    let registry_available = snapshot.is_some();
    if let Some(snapshot) = snapshot.as_ref() {
        add_registry_skill_rows(snapshot, &mut rows);
        add_observed_target_inventory_rows(snapshot, &mut rows, &mut warnings);
        add_observed_import_rows(snapshot, &mut rows);
    } else {
        warnings.push(format!(
            "registry state not initialized under {}",
            paths.registry_dir.display()
        ));
    }
    let trust = paths.load_trust()?;
    add_trust_rows(&trust, &mut rows);

    add_skill_tags(ctx, &mut rows, &mut warnings)?;

    Ok(SkillInventoryReadModel {
        skills: rows.into_values().map(skill_row_to_json).collect(),
        warnings,
        registry_available,
    })
}

fn skill_list_payload(model: &SkillInventoryReadModel) -> Value {
    json!({
        "state_model": "union",
        "registry_available": model.registry_available,
        "count": model.skills.len(),
        "skills": model.skills,
    })
}

fn inventory_meta(warnings: Vec<String>) -> Meta {
    Meta {
        warnings,
        ..Meta::default()
    }
}

fn find_skill<'a>(skills: &'a [Value], skill_id: &str) -> Option<&'a Value> {
    skills
        .iter()
        .find(|skill| skill["skill_id"].as_str() == Some(skill_id))
}

fn map_inventory_error(err: anyhow::Error) -> CommandFailure {
    map_registry_state(err)
}

fn skill_row<'a>(
    rows: &'a mut BTreeMap<String, SkillReadRow>,
    skill_id: &str,
) -> &'a mut SkillReadRow {
    rows.entry(skill_id.to_string())
        .or_insert_with(|| SkillReadRow::new(skill_id.to_string()))
}

fn add_source_skill_rows(
    skills_dir: &Path,
    rows: &mut BTreeMap<String, SkillReadRow>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    if !skills_dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(skills_dir)? {
        let entry = entry?;
        let skill_id = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        let row = skill_row(rows, &skill_id);
        row.sources.insert("source");
        row.source_path = Some(path.clone());
        let lint = lint_skill_source(&path, &skill_id, SkillLintMode::Compat);
        row.entrypoint = lint.entrypoint_path().map(PathBuf::from);
        row.source_status = Some(if path.is_dir() && lint.entrypoint.file_name.is_some() {
            row.description = lint.description().map(str::to_string);
            "present"
        } else {
            "non-compliant"
        });
        row.warnings.extend(
            lint.findings
                .iter()
                .map(|finding| format!("{}: {}", finding.id, finding.message)),
        );
        warnings.extend(
            lint.findings
                .iter()
                .filter(|finding| finding.id == "frontmatter_yaml_invalid")
                .map(|finding| {
                    format!(
                        "failed to read skill description from {}: {}",
                        path.display(),
                        finding.message
                    )
                }),
        );
    }
    Ok(())
}

fn add_registry_skill_rows(snapshot: &RegistrySnapshot, rows: &mut BTreeMap<String, SkillReadRow>) {
    let targets = snapshot
        .targets
        .targets
        .iter()
        .map(|target| (target.target_id.as_str(), target))
        .collect::<BTreeMap<_, _>>();
    let bindings = snapshot
        .bindings
        .bindings
        .iter()
        .map(|binding| (binding.binding_id.as_str(), binding))
        .collect::<BTreeMap<_, _>>();

    for rule in &snapshot.rules.rules {
        let row = skill_row(rows, &rule.skill_id);
        row.sources.insert("rule");
        row.binding_ids.insert(rule.binding_id.clone());
        row.target_ids.insert(rule.target_id.clone());
        if let Some(binding) = bindings.get(rule.binding_id.as_str()) {
            row.profile_ids.insert(binding.profile_id.clone());
            row.compatible_agents.insert(binding.agent.clone());
            row.workspace_matchers.insert(
                binding.binding_id.clone(),
                json!({
                    "binding_id": binding.binding_id,
                    "kind": binding.workspace_matcher.kind,
                    "value": binding.workspace_matcher.value,
                }),
            );
        }
        if let Some(target) = targets.get(rule.target_id.as_str()) {
            add_compatible_target(row, target);
        }
    }

    for projection in &snapshot.projections.projections {
        let row = skill_row(rows, &projection.skill_id);
        row.sources.insert("projection");
        if let Some(binding_id) = projection.binding_id.as_ref() {
            row.binding_ids.insert(binding_id.clone());
            if let Some(binding) = bindings.get(binding_id.as_str()) {
                row.profile_ids.insert(binding.profile_id.clone());
                row.compatible_agents.insert(binding.agent.clone());
            }
        }
        row.target_ids.insert(projection.target_id.clone());
        if let Some(target) = targets.get(projection.target_id.as_str()) {
            add_compatible_target(row, target);
        }
        row.projection_count += 1;
        if !projection.last_applied_rev.is_empty()
            && row.latest_rev.is_none()
            && projection.updated_at.is_none()
        {
            row.latest_rev = Some(projection.last_applied_rev.clone());
        }
        if let Some(updated_at) = projection.updated_at {
            let updated_at = updated_at.to_rfc3339();
            if row
                .latest_updated_at
                .as_ref()
                .is_none_or(|current| updated_at > *current)
            {
                row.latest_updated_at = Some(updated_at);
                row.latest_rev = Some(projection.last_applied_rev.clone());
            }
        }
    }
}

fn add_trust_rows(trust: &RegistryTrustFile, rows: &mut BTreeMap<String, SkillReadRow>) {
    for record in &trust.skills {
        let row = skill_row(rows, &record.skill_id);
        row.trust = Some(record.trust.clone());
        row.quarantined = record.quarantined;
        if record.quarantined {
            row.sources.insert("trust");
        }
    }
}

fn add_compatible_target(row: &mut SkillReadRow, target: &RegistryProjectionTarget) {
    row.compatible_agents.insert(target.agent.clone());
    row.compatible_targets.insert(
        target.target_id.clone(),
        json!({
            "target_id": target.target_id,
            "agent": target.agent,
            "ownership": target.ownership,
            "path": target.path,
        }),
    );
}

fn add_observed_import_rows(
    snapshot: &RegistrySnapshot,
    rows: &mut BTreeMap<String, SkillReadRow>,
) {
    for op in &snapshot.operations {
        if op.intent != "skill.import_observed" && op.intent != "skill.monitor_observed" {
            continue;
        }
        for field in ["imported", "updated"] {
            if let Some(items) = op.effects.get(field).and_then(Value::as_array) {
                for item in items {
                    if let Some(skill_id) = item.get("skill").and_then(Value::as_str) {
                        let row = skill_row(rows, skill_id);
                        row.sources.insert("observed");
                        row.observed_imported = true;
                        if let Some(target_id) = item.get("target_id").and_then(Value::as_str) {
                            row.observed_target_ids.insert(target_id.to_string());
                        }
                    }
                }
            }
        }
        if let Some(items) = op.effects.get("skipped").and_then(Value::as_array) {
            for item in items {
                let reason = item.get("reason").and_then(Value::as_str);
                if reason != Some("already-exists") && reason != Some("duplicate-observed-skill") {
                    continue;
                }
                let Some(skill_id) = item.get("skill").and_then(Value::as_str) else {
                    continue;
                };
                let Some(target_id) = item.get("target_id").and_then(Value::as_str) else {
                    continue;
                };
                let row = skill_row(rows, skill_id);
                row.sources.insert("observed");
                row.observed_imported = true;
                row.observed_target_ids.insert(target_id.to_string());
            }
        }
    }
}

fn add_observed_target_inventory_rows(
    snapshot: &RegistrySnapshot,
    rows: &mut BTreeMap<String, SkillReadRow>,
    warnings: &mut Vec<String>,
) {
    for target in &snapshot.targets.targets {
        if target.ownership != "observed" {
            continue;
        }
        let target_path = PathBuf::from(&target.path);
        if !target_path.exists() {
            warnings.push(format!(
                "observed target {} missing at {}",
                target.target_id,
                target_path.display()
            ));
            continue;
        }
        if !target_path.is_dir() {
            warnings.push(format!(
                "observed target {} is not a directory: {}",
                target.target_id,
                target_path.display()
            ));
            continue;
        }

        let entries = match fs::read_dir(&target_path) {
            Ok(entries) => entries,
            Err(err) => {
                warnings.push(format!(
                    "failed to read observed target {} at {}: {err}",
                    target.target_id,
                    target_path.display()
                ));
                continue;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    warnings.push(format!(
                        "failed to read observed target entry under {}: {err}",
                        target_path.display()
                    ));
                    continue;
                }
            };
            let source_path = entry.path();
            let source = match observed_inventory_source(&source_path) {
                Some(source) => source,
                None => continue,
            };
            let Some(skill_id) = entry.file_name().to_str().map(str::to_string) else {
                warnings.push(format!(
                    "observed target {} contains non-utf8 skill entry {}",
                    target.target_id,
                    source_path.display()
                ));
                continue;
            };
            let lint = lint_skill_source(&source, &skill_id, SkillLintMode::Compat);
            if !lint.compatible {
                continue;
            }
            let row = skill_row(rows, &skill_id);
            row.sources.insert("observed");
            row.observed_imported = true;
            row.observed_target_ids.insert(target.target_id.clone());
            row.description
                .get_or_insert_with(|| lint.description().unwrap_or("").to_string());
            row.entrypoint
                .get_or_insert_with(|| lint.entrypoint_path().map(PathBuf::from).unwrap_or(source));
            add_compatible_target(row, target);
        }
    }
}

fn observed_inventory_source(source_path: &Path) -> Option<PathBuf> {
    let metadata = fs::symlink_metadata(source_path).ok()?;
    if metadata.is_dir() {
        return Some(source_path.to_path_buf());
    }
    if !metadata.file_type().is_symlink() {
        return None;
    }
    let target_metadata = fs::metadata(source_path).ok()?;
    if !target_metadata.is_dir() {
        return None;
    }
    fs::canonicalize(source_path).ok()
}

fn add_skill_tags(
    ctx: &AppContext,
    rows: &mut BTreeMap<String, SkillReadRow>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    if !gitops::repo_is_initialized(ctx)? {
        warnings.push(
            "git repository not initialized; release and snapshot tags unavailable".to_string(),
        );
        return Ok(());
    }

    let output = gitops::run_git_allow_failure(ctx, &["tag", "--list"])?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "failed to list git tags: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    for tag in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(rest) = tag.strip_prefix("release/") {
            if let Some((skill_id, version)) = rest.split_once('/') {
                let row = skill_row(rows, skill_id);
                row.sources.insert("release_tag");
                row.release_tags.push(version.to_string());
            }
        } else if let Some(rest) = tag.strip_prefix("snapshot/")
            && let Some((skill_id, snapshot)) = rest.split_once('/')
        {
            let row = skill_row(rows, skill_id);
            row.sources.insert("snapshot_tag");
            row.snapshot_tags.push(snapshot.to_string());
        }
    }
    Ok(())
}

fn skill_row_to_json(row: SkillReadRow) -> Value {
    let source_status = row.source_status.unwrap_or("missing");
    let binding_ids = row.binding_ids.into_iter().collect::<Vec<_>>();
    let target_ids = row.target_ids.into_iter().collect::<Vec<_>>();
    let observed_target_ids = row.observed_target_ids.into_iter().collect::<Vec<_>>();
    let warnings = row.warnings;
    let trust = row.trust.unwrap_or_else(|| "unknown".to_string());
    let next_actions = next_actions(source_status, &warnings, row.projection_count);
    json!({
        "skill_id": row.skill_id,
        "name": row.skill_id,
        "entrypoint": row.entrypoint.map(|path| path.display().to_string()),
        "description": empty_string_as_null(row.description),
        "source_status": source_status,
        "source_path": row.source_path.map(|path| path.display().to_string()),
        "trust": trust,
        "quarantined": row.quarantined,
        "latest_rev": row.latest_rev,
        "latest_updated_at": row.latest_updated_at,
        "bindings_count": binding_ids.len(),
        "projections_count": row.projection_count,
        "target_ids": target_ids,
        "observed_target_ids": observed_target_ids,
        "release_tags": row.release_tags,
        "snapshot_tags": row.snapshot_tags,
        "observed_imported": row.observed_imported,
        "sources": row.sources.into_iter().collect::<Vec<_>>(),
        "compatible_agents": row.compatible_agents.into_iter().collect::<Vec<_>>(),
        "compatible_targets": row.compatible_targets.into_values().collect::<Vec<_>>(),
        "profile_ids": row.profile_ids.into_iter().collect::<Vec<_>>(),
        "binding_ids": binding_ids,
        "workspace_matchers": row.workspace_matchers.into_values().collect::<Vec<_>>(),
        "warnings": warnings,
        "next_actions": next_actions,
        "projection_summary": {
            "count": row.projection_count,
            "target_ids": target_ids,
            "observed_target_ids": observed_target_ids,
            "latest_rev": row.latest_rev,
            "latest_updated_at": row.latest_updated_at,
        },
    })
}

fn empty_string_as_null(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn next_actions(source_status: &str, warnings: &[String], projection_count: usize) -> Vec<String> {
    let mut actions = Vec::new();
    match source_status {
        "missing" => actions.push("import or add the skill source before projection".to_string()),
        "non-compliant" => {
            actions.push("run `loom skill lint <skill>` and fix source metadata".to_string())
        }
        _ => {}
    }
    if projection_count == 0 {
        actions.push("project the skill into a workspace binding when it is needed".to_string());
    }
    if !warnings.is_empty() {
        actions.push("review inventory warnings before using this skill in automation".to_string());
    }
    actions
}

pub(crate) struct SkillDiscoveryFilters<'a> {
    pub(crate) agent: Option<&'a str>,
    pub(crate) profile: Option<&'a str>,
    pub(crate) status: Option<&'a str>,
    pub(crate) trust: Option<&'a str>,
    pub(crate) workspace: Option<&'a Path>,
}

pub(crate) fn score_and_filter_skills(
    skills: &[Value],
    query: &str,
    filters: SkillDiscoveryFilters<'_>,
    include_workspace_boost: bool,
) -> Vec<Value> {
    let tokens = tokenize(query);
    let mut results = Vec::new();

    for skill in skills {
        if !passes_filters(skill, &filters) {
            continue;
        }
        let (score, inputs) =
            score_skill(skill, &tokens, filters.workspace, include_workspace_boost);
        if score == 0 {
            continue;
        }
        results.push(json!({
            "skill": skill,
            "score": score,
            "score_inputs": inputs,
        }));
    }

    results.sort_by(|a, b| {
        let a_score = a["score"].as_i64().unwrap_or_default();
        let b_score = b["score"].as_i64().unwrap_or_default();
        b_score.cmp(&a_score).then_with(|| {
            a["skill"]["skill_id"]
                .as_str()
                .unwrap_or_default()
                .cmp(b["skill"]["skill_id"].as_str().unwrap_or_default())
        })
    });
    results
}

fn passes_filters(skill: &Value, filters: &SkillDiscoveryFilters<'_>) -> bool {
    if let Some(agent) = filters.agent
        && !value_array_contains(&skill["compatible_agents"], agent)
    {
        return false;
    }
    if let Some(profile) = filters.profile
        && !value_array_contains(&skill["profile_ids"], profile)
    {
        return false;
    }
    if let Some(status) = filters.status
        && skill["source_status"].as_str() != Some(status)
    {
        return false;
    }
    if let Some(trust) = filters.trust
        && skill["trust"].as_str() != Some(trust)
    {
        return false;
    }
    true
}

fn score_skill(
    skill: &Value,
    tokens: &[String],
    workspace: Option<&Path>,
    include_workspace_boost: bool,
) -> (i64, Vec<Value>) {
    let mut score = 0;
    let mut inputs = Vec::new();
    for token in tokens {
        add_field_score(skill, token, "skill_id", 5, &mut score, &mut inputs);
        add_field_score(skill, token, "description", 3, &mut score, &mut inputs);
        add_array_score(skill, token, "release_tags", 2, &mut score, &mut inputs);
        add_array_score(skill, token, "snapshot_tags", 2, &mut score, &mut inputs);
        add_array_score(skill, token, "warnings", 1, &mut score, &mut inputs);
        add_field_score(skill, token, "source_status", 1, &mut score, &mut inputs);
    }
    if include_workspace_boost
        && let Some(workspace) = workspace
        && workspace_matches(skill, workspace)
    {
        score += 4;
        inputs.push(json!({
            "field": "workspace_matchers",
            "weight": 4,
            "reason": "workspace matched a binding matcher",
        }));
    }
    (score, inputs)
}

fn add_field_score(
    skill: &Value,
    token: &str,
    field: &str,
    weight: i64,
    score: &mut i64,
    inputs: &mut Vec<Value>,
) {
    if let Some(value) = skill[field].as_str()
        && value.to_ascii_lowercase().contains(token)
    {
        *score += weight;
        inputs.push(json!({ "field": field, "token": token, "weight": weight }));
    }
}

fn add_array_score(
    skill: &Value,
    token: &str,
    field: &str,
    weight: i64,
    score: &mut i64,
    inputs: &mut Vec<Value>,
) {
    let Some(items) = skill[field].as_array() else {
        return;
    };
    if items.iter().any(|item| {
        item.as_str()
            .is_some_and(|value| value.to_ascii_lowercase().contains(token))
    }) {
        *score += weight;
        inputs.push(json!({ "field": field, "token": token, "weight": weight }));
    }
}

pub(crate) fn workspace_matches(skill: &Value, workspace: &Path) -> bool {
    let workspace = workspace.to_string_lossy();
    let Some(matchers) = skill["workspace_matchers"].as_array() else {
        return false;
    };
    matchers.iter().any(|matcher| {
        let kind = matcher["kind"].as_str();
        let value = matcher["value"].as_str().unwrap_or_default();
        match kind {
            Some("path_prefix") => workspace.starts_with(value),
            Some("exact_path") => workspace == value,
            Some("name") => {
                Path::new(workspace.as_ref())
                    .file_name()
                    .and_then(|name| name.to_str())
                    == Some(value)
            }
            _ => false,
        }
    })
}

fn value_array_contains(value: &Value, expected: &str) -> bool {
    value
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(expected)))
}

pub(crate) fn tokenize(query: &str) -> Vec<String> {
    let mut tokens = query
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        tokens.push(query.trim().to_ascii_lowercase());
    }
    tokens
}
