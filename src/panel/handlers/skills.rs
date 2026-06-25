use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use axum::{Json, extract::Path as AxumPath, extract::State, http::StatusCode};
use serde_json::json;

use crate::cli::SkillOnlyArgs;
use crate::commands::{App, SkillLintMode, lint_skill_source};
use crate::envelope::Envelope;
use crate::gitops;
use crate::state_model::{RegistrySnapshot, RegistryStatePaths};
use crate::types::ErrorCode;

use super::super::PanelState;
use super::common::panel_command_envelope;

#[derive(Debug, Default)]
struct SkillReadRow {
    skill_id: String,
    description: Option<String>,
    source_path: Option<PathBuf>,
    source_status: Option<&'static str>,
    sources: BTreeSet<&'static str>,
    binding_ids: BTreeSet<String>,
    target_ids: BTreeSet<String>,
    observed_target_ids: BTreeSet<String>,
    projection_count: usize,
    latest_rev: Option<String>,
    latest_updated_at: Option<String>,
    release_tags: Vec<String>,
    snapshot_tags: Vec<String>,
    observed_imported: bool,
}

impl SkillReadRow {
    fn new(skill_id: String) -> Self {
        Self {
            skill_id,
            ..Self::default()
        }
    }
}

pub(in crate::panel) async fn v1_skills(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    match build_skill_read_model(&state) {
        Ok((skills, warnings, registry_available)) => (
            StatusCode::OK,
            Json(json!(Envelope::ok(
                "registry.skills",
                uuid::Uuid::new_v4().to_string(),
                json!({
                    "state_model": "union",
                    "registry_available": registry_available,
                    "count": skills.len(),
                    "skills": skills,
                }),
                crate::envelope::Meta {
                    warnings,
                    ..crate::envelope::Meta::default()
                }
            ))),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!(Envelope::err(
                "registry.skills",
                uuid::Uuid::new_v4().to_string(),
                ErrorCode::InternalError,
                err.to_string(),
                serde_json::Value::Object(Default::default())
            ))),
        ),
    }
}

pub(in crate::panel) async fn v1_skill_diagnose(
    AxumPath(skill_name): AxumPath<String>,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let app = App {
        ctx: state.ctx.as_ref().clone(),
    };
    panel_command_envelope(
        "skill.diagnose",
        app.cmd_skill_diagnose(&SkillOnlyArgs { skill: skill_name }),
    )
}

pub(in crate::panel) async fn v1_skill_trash(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let app = App {
        ctx: state.ctx.as_ref().clone(),
    };
    panel_command_envelope("skill.trash.list", app.cmd_skill_trash_list())
}

fn build_skill_read_model(
    state: &PanelState,
) -> anyhow::Result<(Vec<serde_json::Value>, Vec<String>, bool)> {
    let mut warnings = Vec::new();
    let mut rows: BTreeMap<String, SkillReadRow> = BTreeMap::new();

    add_source_skill_rows(&state.ctx.skills_dir, &mut rows, &mut warnings)?;

    let paths = RegistryStatePaths::from_app_context(&state.ctx);
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

    add_skill_tags(state, &mut rows, &mut warnings)?;

    Ok((
        rows.into_values()
            .map(skill_row_to_json)
            .collect::<Vec<_>>(),
        warnings,
        registry_available,
    ))
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
) -> anyhow::Result<()> {
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
        row.source_status = Some(if path.is_dir() && lint.entrypoint.file_name.is_some() {
            row.description = lint.description().map(str::to_string);
            "present"
        } else {
            "non-compliant"
        });
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
    for rule in &snapshot.rules.rules {
        let row = skill_row(rows, &rule.skill_id);
        row.sources.insert("rule");
        row.binding_ids.insert(rule.binding_id.clone());
        row.target_ids.insert(rule.target_id.clone());
    }

    for projection in &snapshot.projections.projections {
        let row = skill_row(rows, &projection.skill_id);
        row.sources.insert("projection");
        if let Some(binding_id) = projection.binding_id.as_ref() {
            row.binding_ids.insert(binding_id.clone());
        }
        row.target_ids.insert(projection.target_id.clone());
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

fn add_observed_import_rows(
    snapshot: &RegistrySnapshot,
    rows: &mut BTreeMap<String, SkillReadRow>,
) {
    for op in &snapshot.operations {
        if op.intent != "skill.import_observed" && op.intent != "skill.monitor_observed" {
            continue;
        }
        for field in ["imported", "updated"] {
            if let Some(items) = op.effects.get(field).and_then(serde_json::Value::as_array) {
                for item in items {
                    if let Some(skill_id) = item.get("skill").and_then(serde_json::Value::as_str) {
                        let row = skill_row(rows, skill_id);
                        row.sources.insert("observed");
                        row.observed_imported = true;
                        if let Some(target_id) =
                            item.get("target_id").and_then(serde_json::Value::as_str)
                        {
                            row.observed_target_ids.insert(target_id.to_string());
                        }
                    }
                }
            }
        }
        if let Some(items) = op
            .effects
            .get("skipped")
            .and_then(serde_json::Value::as_array)
        {
            for item in items {
                let reason = item.get("reason").and_then(serde_json::Value::as_str);
                if reason != Some("already-exists") && reason != Some("duplicate-observed-skill") {
                    continue;
                }
                let Some(skill_id) = item.get("skill").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                let Some(target_id) = item.get("target_id").and_then(serde_json::Value::as_str)
                else {
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
    state: &PanelState,
    rows: &mut BTreeMap<String, SkillReadRow>,
    warnings: &mut Vec<String>,
) -> anyhow::Result<()> {
    if !gitops::repo_is_initialized(&state.ctx)? {
        warnings.push(
            "git repository not initialized; release and snapshot tags unavailable".to_string(),
        );
        return Ok(());
    }

    let output = gitops::run_git_allow_failure(&state.ctx, &["tag", "--list"])?;
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

fn skill_row_to_json(row: SkillReadRow) -> serde_json::Value {
    let source_status = row.source_status.unwrap_or("missing");
    json!({
        "skill_id": row.skill_id,
        "description": row.description,
        "source_status": source_status,
        "source_path": row.source_path.map(|path| path.display().to_string()),
        "latest_rev": row.latest_rev,
        "latest_updated_at": row.latest_updated_at,
        "bindings_count": row.binding_ids.len(),
        "projections_count": row.projection_count,
        "target_ids": row.target_ids.into_iter().collect::<Vec<_>>(),
        "observed_target_ids": row.observed_target_ids.into_iter().collect::<Vec<_>>(),
        "release_tags": row.release_tags,
        "snapshot_tags": row.snapshot_tags,
        "observed_imported": row.observed_imported,
        "sources": row.sources.into_iter().collect::<Vec<_>>(),
    })
}
